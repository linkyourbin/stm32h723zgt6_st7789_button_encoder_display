#![no_std]
#![no_main]

use defmt::{error, info};
use embassy_executor::Spawner;
use embassy_stm32::gpio::Input;
use embassy_stm32::spi::{Config as SpiConfig, Spi};
use embassy_stm32::timer::qei::{Qei, QeiPin};
use embassy_stm32::{
    gpio::{Level, Output, Pull, Speed},
    time::Hertz,
};
use embassy_time::{Delay, Timer};
use {defmt_rtt as _, panic_probe as _};

// SPI 显示接口
use display_interface_spi::SPIInterface;
// ST7789 驱动
use st7789::{BacklightState, Orientation, ST7789};
// Embedded Graphics
use embedded_graphics::{
    image::{Image, ImageRawLE},
    mono_font::{MonoTextStyle, ascii::FONT_6X13_ITALIC},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::*,
    text::Text,
};

// 屏幕尺寸常量（240x240）
const SCREEN_WIDTH: i32 = 240;
const SCREEN_HEIGHT: i32 = 240;
// 按键消抖延迟（毫秒）
const DEBOUNCE_DELAY: u64 = 20;
// 按键按下检测间隔
const KEY_SCAN_INTERVAL: u64 = 50;
// 图片总数（包括默认图）
const TOTAL_IMAGES: u8 = 5;

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let mut peripheral_config = embassy_stm32::Config::default();
    {
        use embassy_stm32::rcc::*;
        peripheral_config.rcc.hse = Some(Hse {
            freq: Hertz(25_000_000),
            mode: HseMode::Oscillator,
        });
        peripheral_config.rcc.pll1 = Some(Pll {
            source: PllSource::HSE,
            prediv: PllPreDiv::DIV5,
            mul: PllMul::MUL160,
            divp: Some(PllDiv::DIV2),
            divq: Some(PllDiv::DIV2),
            divr: Some(PllDiv::DIV2),
        });
        peripheral_config.rcc.sys = Sysclk::PLL1_P;
        peripheral_config.rcc.ahb_pre = AHBPrescaler::DIV2;
        peripheral_config.rcc.apb1_pre = APBPrescaler::DIV2;
        peripheral_config.rcc.apb2_pre = APBPrescaler::DIV2;
        peripheral_config.rcc.apb3_pre = APBPrescaler::DIV2;
        peripheral_config.rcc.apb4_pre = APBPrescaler::DIV2;

        peripheral_config.rcc.mux.spdifrxsel = mux::Spdifrxsel::PLL1_Q;
    }
    let p = embassy_stm32::init(peripheral_config);

    info!("hello rust");

    // 配置SPI
    let mut spi_config = SpiConfig::default();
    spi_config.frequency = Hertz(50_000_000);

    // 设置 SPI 模式为 Mode 3
    spi_config.mode = embassy_stm32::spi::Mode {
        polarity: embassy_stm32::spi::Polarity::IdleHigh,
        phase: embassy_stm32::spi::Phase::CaptureOnSecondTransition,
    };

    let spi = Spi::new_txonly(p.SPI1, p.PA5, p.PA7, p.DMA2_CH2, spi_config);

    // 控制引脚
    let cs = Output::new(p.PA4, Level::High, Speed::VeryHigh);
    let dc = Output::new(p.PA2, Level::Low, Speed::VeryHigh);
    let rst = Output::new(p.PA6, Level::High, Speed::VeryHigh);
    let bl = Output::new(p.PA1, Level::Low, Speed::VeryHigh);

    // 创建显示接口
    let di = SPIInterface::new(spi, dc, cs);

    // 创建ST7789驱动实例
    let mut display = ST7789::new(
        di,
        Some(rst),
        Some(bl),
        SCREEN_WIDTH as u16,
        SCREEN_HEIGHT as u16,
    );

    // 初始化延迟
    let mut delay = Delay;

    // 初始化显示屏
    info!("初始化ST7789...");
    match display.init(&mut delay) {
        Ok(_) => info!("初始化成功"),
        Err(_) => {
            error!("初始化ST7789失败");
            loop {}
        }
    }

    // 设置方向为横屏
    if let Err(_) = display.set_orientation(Orientation::Portrait) {
        error!("设置屏幕方向失败");
    } else {
        info!("方向设置为横屏");
    }

    // 打开背光
    if let Err(_) = display.set_backlight(BacklightState::On, &mut delay) {
        error!("打开屏幕背光失败");
    } else {
        info!("背光已打开");
    }

    // 按键初始化
    let key1 = Input::new(p.PF12, Pull::Up);
    let key2 = Input::new(p.PF13, Pull::Up);
    let key3 = Input::new(p.PF14, Pull::Up);
    let key4 = Input::new(p.PF15, Pull::Up);

    let mut current_image = 0; // 当前显示的图像索引

    // 初始显示默认图像
    show_image(&mut display, current_image).await;

    // 配置旋转编码器引脚
    let encoder_ch1_pin = QeiPin::new_ch1(p.PE9);
    let encoder_ch2_pin = QeiPin::new_ch2(p.PE11);

    // 创建QEI实例
    let mut qei = Qei::new(p.TIM1, encoder_ch1_pin, encoder_ch2_pin);

    // 获取初始位置
    let mut last_position = qei.count() as i32;
    info!("初始位置: {}", last_position);

    // 主循环 - 按键扫描与图像显示
    loop {
        // 检测按键1
        if key1.get_level() == Level::Low {
            Timer::after_millis(DEBOUNCE_DELAY).await;
            if key1.get_level() == Level::Low {
                info!("按键1按下 - 显示图片1");
                current_image = 1;
                show_image(&mut display, current_image).await;
                // 等待按键释放
                while key1.get_level() == Level::Low {
                    Timer::after_millis(KEY_SCAN_INTERVAL).await;
                }
            }
        }
        // 检测按键2
        else if key2.get_level() == Level::Low {
            Timer::after_millis(DEBOUNCE_DELAY).await;
            if key2.get_level() == Level::Low {
                info!("按键2按下 - 显示图片2");
                current_image = 2;
                show_image(&mut display, current_image).await;
                while key2.get_level() == Level::Low {
                    Timer::after_millis(KEY_SCAN_INTERVAL).await;
                }
            }
        }
        // 检测按键3
        else if key3.get_level() == Level::Low {
            Timer::after_millis(DEBOUNCE_DELAY).await;
            if key3.get_level() == Level::Low {
                info!("按键3按下 - 显示图片3");
                current_image = 3;
                show_image(&mut display, current_image).await;
                while key3.get_level() == Level::Low {
                    Timer::after_millis(KEY_SCAN_INTERVAL).await;
                }
            }
        }
        // 检测按键4
        else if key4.get_level() == Level::Low {
            Timer::after_millis(DEBOUNCE_DELAY).await;
            if key4.get_level() == Level::Low {
                info!("按键4按下 - 显示图片4");
                current_image = 4;
                show_image(&mut display, current_image).await;
                while key4.get_level() == Level::Low {
                    Timer::after_millis(KEY_SCAN_INTERVAL).await;
                }
            }
        }

        // 旋转编码器处理
        let current_position = qei.count() as i32;
        let position_diff = current_position - last_position;

        // 检测到有效旋转（差值大于1或小于-1，防止抖动）
        if position_diff > 1 {
            info!("向右旋转: {}", position_diff);
            // 显示下一张图片，循环处理
            current_image = (current_image + 1) % TOTAL_IMAGES;
            show_image(&mut display, current_image).await;
            last_position = current_position;
        } else if position_diff < -1 {
            info!("向左旋转: {}", position_diff);
            // 显示上一张图片，循环处理
            current_image = (current_image + TOTAL_IMAGES - 1) % TOTAL_IMAGES;
            show_image(&mut display, current_image).await;
            last_position = current_position;
        }

        // 短暂延迟，降低CPU占用
        Timer::after_millis(KEY_SCAN_INTERVAL).await;
    }
}

/// 图像显示函数
async fn show_image(
    display: &mut impl embedded_graphics::draw_target::DrawTarget<Color = Rgb565>,
    image_idx: u8,
) {
    // 根据索引选择图像
    let (image_data, width) = match image_idx {
        1 => (include_bytes!("../assets/mode_0/1.raw"), 240),
        2 => (include_bytes!("../assets/mode_0/2.raw"), 240),
        3 => (include_bytes!("../assets/mode_0/3.raw"), 240),
        4 => (include_bytes!("../assets/mode_0/4.raw"), 240),
        _ => (include_bytes!("../assets/mode_0/avatar.raw"), 240),
    };

    // 绘制图像
    let raw_image = ImageRawLE::<Rgb565>::new(image_data, width);
    let _ = Image::new(&raw_image, Point::new(0, 0)).draw(display);
}
