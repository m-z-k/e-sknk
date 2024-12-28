#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

use core::borrow::Borrow;

use ch32_hal::peripherals::TIM1;
use ch32_hal::time::Hertz;
use ch32_hal::timer::low_level::CountingMode;
use ch32_hal::timer::simple_pwm::{PwmPin, SimplePwm};
use ch32_hal::Peripherals;
use hal::delay::Delay;
use hal::gpio::{Level, Output};
use {ch32_hal as hal, panic_halt as _};

//----------------------------------------------------------------------------

/// デシリアライザ(74HC595)
struct Des<'a> {
    data: Output<'a>,
    clock: Output<'a>,
    latch: Output<'a>,
}

impl<'a> Des<'a> {
    /// ピンの定義から構造体を作成
    fn from_pins(data: Output<'a>, clock: Output<'a>, latch: Output<'a>) -> Des<'a> {
        Des { data, clock, latch }
    }

    /// 8ビットの値を即時セットする
    fn update(&mut self, val: u8) {
        let total = 10000;
        let w = total / 3 / 9;
        // ピンに正のパルスを出力する
        let tick = |pin: &mut Output, d: u32| {
            let mut delay = Delay;
            delay.delay_us(d);
            pin.set_high();
            delay.delay_us(d);
            pin.set_low();
            delay.delay_us(d);
        };

        for i in 0..8 {
            let bit = (val & (1u8 << (7 - i))) != 0;
            if bit {
                self.data.set_high();
            } else {
                self.data.set_low();
            }
            tick(&mut self.clock, w);
        }
        tick(&mut self.latch, w);
    }
}

//----------------------------------------------------------------------------

/// PWMの初期化
fn init_pwm(p: &Peripherals) -> SimplePwm<TIM1> {
    let pin = PwmPin::new_ch2::<0>(p.PA1.borrow());
    let mut pwm = SimplePwm::new(
        p.TIM1.borrow(),
        None,
        Some(pin),
        None,
        None,
        Hertz::hz(1),
        CountingMode::default(),
    );
    let ch = hal::timer::Channel::Ch2;
    pwm.set_frequency(Hertz::khz(20));
    pwm.set_duty(ch, pwm.get_max_duty() / 2);
    pwm.enable(ch);

    return pwm;
}

//----------------------------------------------------------------------------

enum Sequence {
    End,
    Wait(u16 /*ms */),
    Note(u8),
    Led(u8),
}

//----------------------------------------------------------------------------

const NOTES: [u16; 128] = [
    40000, 1000, 2000, 3000, 10, 11, 12, 12, 13, 14, 15, 15, // -1
    16, 17, 18, 19, 21, 22, 23, 24, 26, 28, 29, 31, // 0
    33, 35, 37, 39, 41, 44, 46, 49, 52, 55, 58, 62, //+1
    65, 69, 73, 78, 82, 87, 92, 98, 104, 110, 117, 123, // +2
    131, 139, 147, 156, 165, 175, 185, 196, 208, 220, 233, 247, // +3
    262, 277, 294, 311, 330, 349, 370, 392, 415, 440, 466, 494, // +4
    523, 554, 587, 622, 659, 698, 740, 784, 831, 880, 932, 988, // +5
    1047, 1109, 1175, 1245, 1319, 1397, 1480, 1568, 1661, 1760, 1865, 1976, //+6
    2093, 2217, 2349, 2489, 2637, 2794, 2960, 3136, 3322, 3520, 3729, 3951, // +7
    4186, 4435, 4699, 4978, 5274, 5588, 5920, 6272, 6645, 7040, 7459, 7902, //+8
    8372, 8870, 9397, 9956, 10548, 11175, 11840, 12544, // +9
];

// seqs の index 番目からのシーケンスを実行する。
// もし End を見つけたらシーケンスの実行を中断して End の次の要素のインデックス番号を返す。
// seqs の末尾の要素以降には End が無限に続いているものとみなす。
fn exec(p: &Peripherals, seqs: &[Sequence], index: usize) -> usize {
    let mut pwm = init_pwm(&p);

    let mut des = Des::from_pins(
        Output::new(p.PC1.borrow(), Level::Low, Default::default()),
        Output::new(p.PC2.borrow(), Level::Low, Default::default()),
        Output::new(p.PC4.borrow(), Level::Low, Default::default()),
    );

    let mut delay = Delay;
    let mut i = index;

    loop {
        let s = seqs.get(i).unwrap_or(&Sequence::End);
        i += 1;
        match s {
            Sequence::End => break,
            Sequence::Wait(ms) => delay.delay_ms(*ms as u32),
            Sequence::Note(x) => {
                let mut n = x;
                // if *x != 0 {
                //     n = &1;
                // }
                if let Some(freq) = NOTES.get(*n as usize) {
                    pwm.set_frequency(Hertz::hz(*freq as u32));
                    pwm.set_duty(hal::timer::Channel::Ch2, pwm.get_max_duty() / 2);
                }
            }
            Sequence::Led(pattern) => des.update(*pattern),
        }
    }
    let ch = hal::timer::Channel::Ch2;
    pwm.disable(ch);

    i
}
//----------------------------------------------------------------------------
// マルコフ連鎖

struct Edge {
    id: u8,
    weight: u8,
}

impl Edge {
    fn new(id: u8, weight: u8) -> Edge {
        Edge { id, weight }
    }
}

struct Node<'a> {
    id: u8,
    s: char,
    edges: &'a [Edge],
}

fn next_id(now_id: u8, nodes: &[Node], rand: &mut u16) -> u8 {
    for n in nodes {
        if n.id != now_id {
            continue;
        }
        // 遷移確率の分母を求める
        let total: i32 = n.edges.iter().map(|e| e.weight as i32).sum();

        // 乱数を用意する
        *rand = next_rand(*rand);

        // 乱数値を使って遷移先を決定する
        let r = (*rand % (total as u16)) as u32;
        let mut upper = 0;
        for e in n.edges {
            upper += e.weight as u32;
            if r < upper {
                return e.id;
            }
        }
    }
    return now_id;
}
//----------------------------------------------------------------------------
fn adc<'a>(p: &'a mut Peripherals) -> u16 {
    let mut adc = hal::adc::Adc::new(p.ADC1.borrow(), Default::default());
    let pin = &mut p.PA2;
    let ret = adc.convert(pin, hal::adc::SampleTime::CYCLES73);
    return ret;
}

//----------------------------------------------------------------------------
// 乱数

fn xorshift16(x: u16) -> u16 {
    let mut x = x;
    x ^= x << 7;
    x ^= x >> 9;
    x ^= x << 8;
    x
}
fn txs16(x: u16) -> u16 {
    let mut x = x;
    x = xorshift16(x);
    x = xorshift16(x);
    x = xorshift16(x);
    x
}

fn init_rand<'a>(p: &'a mut Peripherals) -> u16 {
    // メモリーに直接アクセスして乱数の種を得る
    let mut seed: u16 = 0;
    unsafe {
        for offset in (0..2048).step_by(2) {
            let addr = 0x20000000 + offset;
            let p = addr as *const u16;
            seed = seed * 97 + *p;
        }
    }
    let mut gen = || -> u16 {
        let a = adc(p);
        txs16(a)
    };
    seed ^ gen() ^ gen() ^ gen()
}

fn update_rand<'a>(rand: u16, p: &'a mut Peripherals) -> u16 {
    txs16(rand ^ (txs16(adc(p)) & 0x0f))
}

fn next_rand(rand: u16) -> u16 {
    txs16(rand)
}

#[qingke_rt::entry]
fn main() -> ! {
    let mut config = hal::Config::default();
    config.rcc = hal::rcc::Config::SYSCLK_FREQ_48MHZ_HSI;
    let p = &mut hal::init(config);

    let mut rand = init_rand(p);

    {
        // ピポ音
        let pipo = [
            Sequence::Led(0),
            Sequence::Led(0),
            Sequence::Note(2),
            Sequence::Wait(150),
            Sequence::Note(1),
            Sequence::Wait(150),
            Sequence::Note(0),
        ];

        // VVVF音
        let vw = 150;
        let vvvf = [
            // 63 65 67 69 70 72 74 75 77 79
            Sequence::Led(0),
            Sequence::Led(0),
            Sequence::Note(63),
            Sequence::Wait(vw * 5),
            Sequence::Note(65),
            Sequence::Wait(vw),
            Sequence::Note(67),
            Sequence::Wait(vw),
            Sequence::Note(69),
            Sequence::Wait(vw),
            Sequence::Note(70),
            Sequence::Wait(vw),
            Sequence::Note(72),
            Sequence::Wait(vw),
            Sequence::Note(74),
            Sequence::Wait(vw),
            Sequence::Note(75),
            Sequence::Wait(vw),
            Sequence::Note(77),
            Sequence::Wait(vw),
            Sequence::Note(79),
            Sequence::Wait(vw * 17),
            Sequence::Note(0),
        ];

        let w = 50;
        let round = [
            //
            Sequence::Wait(w),
            Sequence::Led(1),
            Sequence::Wait(w),
            Sequence::Led(2),
            Sequence::Wait(w),
            Sequence::Led(4),
            Sequence::Wait(w),
            Sequence::Led(8),
            Sequence::Wait(w),
            //
            Sequence::Led(16),
            Sequence::Wait(w),
            Sequence::Led(32),
            Sequence::Wait(w),
            Sequence::Led(64),
            Sequence::Wait(w),
            Sequence::Led(128),
            Sequence::Wait(w),
            Sequence::Led(0),
            //
            Sequence::Note(0),
            Sequence::Wait(200),
        ];

        rand = update_rand(rand, p);
        match rand % 11 {
            3 => exec(p, &vvvf, 0),
            9 => exec(p, &vvvf, 0),
            10 => exec(p, &vvvf, 0),
            _ => exec(p, &pipo, 0),
        };
        exec(p, &round, 0);
    }

    // しかのこ状態遷移機械
    let shika_nodes: [Node; 9] = [
        // 始点
        Node {
            id: 0,
            s: '○',
            edges: &[Edge::new(1, 1)],
        },
        Node {
            id: 1,
            s: 'ぬ',
            edges: &[Edge::new(2, 1)],
        },
        Node {
            id: 2,
            s: 'ん',
            edges: &[Edge::new(3, 2), Edge::new(8, 3)],
        },
        Node {
            id: 3,
            s: 'た',
            edges: &[Edge::new(2, 1)],
        },
        Node {
            id: 4,
            s: 'か',
            edges: &[Edge::new(5, 1)],
        },
        Node {
            id: 5,
            s: 'の',
            edges: &[Edge::new(6, 1)],
        },
        Node {
            id: 6,
            s: 'こ',
            edges: &[Edge::new(5, 1), Edge::new(7, 1), Edge::new(8, 2)],
        },
        Node {
            id: 7,
            s: 'し',
            edges: &[Edge::new(3, 1), Edge::new(4, 1)],
        },
        Node {
            id: 8,
            s: '＿',
            edges: &[Edge::new(1, 1), Edge::new(5, 4), Edge::new(6, 4), Edge::new(7, 4)],
        },
    ];

    let o = 163; // 1音分の時間
    let l = 10; // LED書き込みの時間
    let s = 80; //音を鳴らしてる時間
    let w = 40; //間に挟まる＿の時間

    // let o = 244; // 1音分の時間
    // let l = 10; // LED書き込みの時間
    // let s = 120; //音を鳴らしてる時間
    // let w = 60; //間に挟まる＿の時間

    let shika_oneshot_head = [
        // 初期状態
        Sequence::Led(128),
        Sequence::Note(0),
        Sequence::Wait(o * 2 - l),
        // ぬん|＿
        Sequence::Led(1), //ぬ
        Sequence::Note(2),
        Sequence::Wait(s / 2 - l),
        Sequence::Led(2), //ん
        Sequence::Wait(s / 2),
        Sequence::Note(0),
        Sequence::Wait(o - l - s),
        Sequence::Led(128), //＿
        Sequence::Wait(o - l),
    ];
    let shika_oneshot_body = [
        // し|か
        Sequence::Led(64), //し
        Sequence::Note(2),
        Sequence::Wait(s),
        Sequence::Note(0),
        Sequence::Wait(o - l - s),
        Sequence::Led(8), //か
        Sequence::Note(1),
        Sequence::Wait(s),
        Sequence::Note(0),
        Sequence::Wait(o - l - s),
        // の|こ|(＿)
        Sequence::Led(16), //の
        Sequence::Note(1),
        Sequence::Wait(s),
        Sequence::Note(0),
        Sequence::Wait(o - l - s),
        Sequence::Led(32), //こ
        Sequence::Note(1),
        Sequence::Wait(s),
        Sequence::Note(0),
        Sequence::Wait(o - l - s - w),
        Sequence::Led(128), //＿
        Sequence::Wait(w - l),
        // の|こ
        Sequence::Led(16), //の
        Sequence::Note(2),
        Sequence::Wait(s),
        Sequence::Note(0),
        Sequence::Wait(o - l - s),
        Sequence::Led(32), //こ
        Sequence::Note(1),
        Sequence::Wait(s),
        Sequence::Note(0),
        Sequence::Wait(o - l - s),
        // の|こ|(＿)
        Sequence::Led(16), //の
        Sequence::Note(2),
        Sequence::Wait(s),
        Sequence::Note(0),
        Sequence::Wait(o - l - s),
        Sequence::Led(32), //こ
        Sequence::Note(1),
        Sequence::Wait(s),
        Sequence::Note(0),
        Sequence::Wait(o - l - s - w),
        Sequence::Led(128), //＿
        Sequence::Wait(w - l),
        // こし|x
        Sequence::Led(32), //こ
        Sequence::Note(2),
        Sequence::Wait(s / 2 - l),
        Sequence::Led(64), //し
        Sequence::Wait(s / 2),
        Sequence::Note(0),
        Sequence::Wait(o * 2 - s - l),
        // たん|x
        Sequence::Led(4), //た
        Sequence::Note(1),
        Sequence::Wait(s / 2 - l),
        Sequence::Led(2), //ん
        Sequence::Wait(s / 2 - l),
        Sequence::Note(0),
        Sequence::Wait(o * 2 - s - l),
        // たん|x
        Sequence::Led(4), //た
        Sequence::Note(1),
        Sequence::Wait(s / 2 - l),
        Sequence::Led(2), //ん
        Sequence::Wait(s / 2),
        Sequence::Note(0),
        Sequence::Wait(o * 2 - s - l),
    ];
    let shika_oneshot_spacer = [
        // ＿|＿
        Sequence::Led(128), //＿
        Sequence::Wait(o * 2 - l),
    ];
    let shika_oneshot_tail = [
        // ぬん|＿
        Sequence::Led(1), //ぬ
        Sequence::Note(3),
        Sequence::Wait(s / 2 - l),
        Sequence::Led(2), //ん
        Sequence::Wait(s / 2),
        Sequence::Note(0),
        Sequence::Wait(o - l - s),
        Sequence::Led(128), //＿
        Sequence::Wait(o - l),
        Sequence::Led(0), //消灯
        Sequence::Wait(o * 4 - l),
    ];

    rand = update_rand(rand, p);
    if rand % 97 < (97 / 2) {
        // イントロ再生
        let seq = [Sequence::Led(0x0e), Sequence::Wait(200), Sequence::Led(0)];
        exec(p, &seq, 0);

        loop {
            exec(p, &shika_oneshot_head, 0);
            for _ in 0..3 {
                exec(p, &shika_oneshot_body, 0);
                exec(p, &shika_oneshot_spacer, 0);
            }
            exec(p, &shika_oneshot_body, 0);
            exec(p, &shika_oneshot_tail, 0);
        }
    } else {
        // マルコフ連鎖モード
        let seq = [Sequence::Led(0xe0), Sequence::Wait(200), Sequence::Led(0)];
        exec(p, &seq, 0);

        let mut id: u8 = 0;
        loop {
            rand = update_rand(rand, p);
            id = next_id(id, &shika_nodes, &mut rand);
            let pat = match id {
                0 => 0x00,
                i => 1u8 << (i - 1),
            };

            if id == 1 {
                let seq = [
                    Sequence::Led(pat),
                    Sequence::Note(3),
                    Sequence::Wait(s),
                    Sequence::Note(0),
                    Sequence::Wait(o - s - l),
                ];
                exec(p, &seq, 0);
            } else if id == 3 || id == 4 || id == 5 || id == 8 {
                let seq = [
                    Sequence::Led(pat),
                    Sequence::Note(2),
                    Sequence::Wait(s),
                    Sequence::Note(0),
                    Sequence::Wait(o - s - l),
                ];
                exec(p, &seq, 0);
            } else {
                let seq = [
                    Sequence::Led(pat),
                    Sequence::Note(1),
                    Sequence::Wait(s),
                    Sequence::Note(0),
                    Sequence::Wait(o - s - l),
                    Sequence::Wait(o),
                ];
                exec(p, &seq, 0);
            }
        }
    }
}
