mod core;

use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContext, EguiPlugin};
use egui_file_dialog::FileDialog;
use std::fs;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::HeapRb;

// Audio output stream wrapper
#[derive(Resource)]
pub struct AudioStream {
    pub producer: ringbuf::Producer<f32, std::sync::Arc<HeapRb<f32>>>,
    pub sample_rate: f32,
    pub channels: u16,
}

// use std::io::Write;

use crate::core::bus::Bus;
use crate::core::cartridge::Cartridge;
use crate::core::cpu::Cpu;
use crate::core::joypad::JoypadButton;
use crate::core::ppu::{SCREEN_WIDTH, SCREEN_HEIGHT, OPAQUE_ALPHA, RENDER_SCALE};

/// CPU Clock frequency for NTSC NES (1.789773 MHz)
pub const NTSC_CPU_CLOCK: f32 = 1_789_773.0;
/// The ratio of PPU cycles per CPU cycle (3:1 for NTSC)
pub const PPU_CPU_CYCLE_RATIO: u32 = 3;
/// Target audio buffer headroom (number of free samples to maintain)
pub const AUDIO_BUFFER_HEADROOM: usize = 1024;
/// Internal buffer size for the audio ringbuffer
pub const AUDIO_RINGBUFFER_SIZE: usize = 4096;

// The central emulator resource
#[derive(Resource)]
struct NesEmulator {
    cpu: Cpu,
    bus: Bus,
    running: bool,
}

impl NesEmulator {
    fn new(cartridge: Cartridge) -> Self {
        NesEmulator {
            cpu: Cpu::new(),
            bus: Bus::new(cartridge),
            running: true,
        }
    }
}

// Resource to hold the UI File Dialog state
#[derive(Resource)]
struct UiState {
    file_dialog: FileDialog,
}

// Component to mark the sprite that displays the emulator output
#[derive(Component)]
struct ScreenSprite;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()).set(WindowPlugin {
            primary_window: Some(Window {
                title: "AINES".into(),
                resolution: ((SCREEN_WIDTH as f32 * RENDER_SCALE) as u32, (SCREEN_HEIGHT as f32 * RENDER_SCALE) as u32).into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .insert_resource(UiState {
            file_dialog: FileDialog::new(),
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (ui_system, emulator_system))
        .run();
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
) {
    // Set up a 2D camera
    commands.spawn(Camera2d);

    // Create a 256x240 image for the NES screen
    let size = Extent3d {
        width: SCREEN_WIDTH as u32,
        height: SCREEN_HEIGHT as u32,
        ..default()
    };
    
    // Create an empty image buffer filled with black
    let image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, OPAQUE_ALPHA],
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::MAIN_WORLD | bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    
    // Add it to Bevy's asset system
    let image_handle = images.add(image);

    // Spawn a sprite to display the image
    commands.spawn((
        Sprite {
            image: image_handle,
            ..default()
        },
        Transform::from_scale(Vec3::new(RENDER_SCALE, RENDER_SCALE, 1.0)),
        ScreenSprite,
    ));

    // Initialize Audio
    let host = cpal::default_host();
    let device = host.default_output_device().expect("no output device available");
    let config = device.default_output_config().unwrap();
    let sample_rate = config.sample_rate().0 as f32;
    let channels = config.channels();
    
    let rb = HeapRb::<f32>::new(AUDIO_RINGBUFFER_SIZE);
    let (producer, mut consumer) = rb.split();
    
    let _stream = device.build_output_stream(
        &config.into(),
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            for sample in data.iter_mut() {
                *sample = consumer.pop().unwrap_or(0.0);
            }
        },
        |err| eprintln!("audio stream error: {}", err),
        None
    ).unwrap();
    _stream.play().unwrap();

    // We must keep the stream alive. Leaking it is acceptable for a singleton.
    std::mem::forget(_stream);

    commands.insert_resource(AudioStream { producer, sample_rate, channels });

    // Load default ROM
    let default_rom_path = "src/assets/Super Mario Bros. (World).nes";
    match fs::read(default_rom_path) {
        Ok(data) => {
            match Cartridge::load_rom(&data) {
                Ok(cartridge) => {
                    println!("Successfully loaded default ROM! Mapper: {}", cartridge.mapper);
                    let mut emulator = NesEmulator::new(cartridge);
                    emulator.cpu.reset(&mut emulator.bus);
                    emulator.running = true;
                    commands.insert_resource(emulator);
                }
                Err(e) => eprintln!("Failed to parse default ROM: {}", e),
            }
        }
        Err(e) => eprintln!("Failed to read default ROM at {}: {}", default_rom_path, e),
    }
}

fn ui_system(
    mut q_egui: Query<&mut EguiContext, With<PrimaryWindow>>,
    mut ui_state: ResMut<UiState>,
    mut commands: Commands,
) {
    for mut egui_context in q_egui.iter_mut() {
        let ctx = egui_context.get_mut();
        // egui menu removed for now
        
        // Update the file dialog
        ui_state.file_dialog.update(ctx);

        if let Some(path) = ui_state.file_dialog.take_picked() {
            println!("Selected file: {:?}", path);
            match fs::read(path) {
                Ok(data) => {
                    match Cartridge::load_rom(&data) {
                        Ok(cartridge) => {
                            println!("Successfully loaded ROM! Mapper: {}", cartridge.mapper);
                            let mut emulator = NesEmulator::new(cartridge);
                            emulator.cpu.reset(&mut emulator.bus);
                            emulator.running = true;
                            commands.insert_resource(emulator);
                        }
                        Err(e) => {
                            eprintln!("Failed to parse ROM: {}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to read file: {}", e);
                }
            }
        }
    }
}
fn emulator_system(
    emulator: Option<ResMut<NesEmulator>>,
    mut images: ResMut<Assets<Image>>,
    query: Query<&Sprite, With<ScreenSprite>>,
    keyboard_input: Res<ButtonInput<KeyCode>>,
    _time: Res<Time>,
    mut audio: ResMut<AudioStream>,
) {
    let mut emu = match emulator {
        Some(e) => e,
        None => return, // Emulator hasn't been loaded yet
    };

    if emu.running {
        // Handle Input
        emu.bus.joypad1.set_button_pressed_status(JoypadButton::UP, keyboard_input.pressed(KeyCode::ArrowUp));
        emu.bus.joypad1.set_button_pressed_status(JoypadButton::DOWN, keyboard_input.pressed(KeyCode::ArrowDown));
        emu.bus.joypad1.set_button_pressed_status(JoypadButton::LEFT, keyboard_input.pressed(KeyCode::ArrowLeft));
        emu.bus.joypad1.set_button_pressed_status(JoypadButton::RIGHT, keyboard_input.pressed(KeyCode::ArrowRight));
        
        // A button: Z (QWERTY), W (AZERTY), or ArrowUp (for jumping)
        let a_pressed = keyboard_input.pressed(KeyCode::KeyZ) || keyboard_input.pressed(KeyCode::KeyW) || keyboard_input.pressed(KeyCode::ArrowUp);
        emu.bus.joypad1.set_button_pressed_status(JoypadButton::BUTTON_A, a_pressed);
        
        emu.bus.joypad1.set_button_pressed_status(JoypadButton::BUTTON_B, keyboard_input.pressed(KeyCode::KeyX));
        
        if keyboard_input.just_pressed(KeyCode::Enter) {
            println!("START button pressed!");
        }
        emu.bus.joypad1.set_button_pressed_status(JoypadButton::START, keyboard_input.pressed(KeyCode::Enter));
        emu.bus.joypad1.set_button_pressed_status(JoypadButton::SELECT, keyboard_input.pressed(KeyCode::ShiftLeft));

        let mut updated = false;

        // Sampling rate tracking
        static mut SAMPLE_ACCUMULATOR: f32 = 0.0;
        let sample_step = NTSC_CPU_CLOCK / audio.sample_rate; // Hardware-accurate ratio

        // Audio-driven sync: Run emulator until audio buffer is sufficiently full
        // We want to keep about 2048-3072 samples in the 4096 buffer
        while audio.producer.free_len() > AUDIO_BUFFER_HEADROOM {
            let mut _frame_complete = false;
            let NesEmulator { cpu, bus, .. } = &mut *emu;

            while !_frame_complete {
                let cycles = cpu.step(bus);
                for _ in 0..cycles {
                    bus.apu.step();
                    
                    // SAFETY: SAMPLE_ACCUMULATOR is a static mut used for high-fidelity audio 
                    // resample tracking. Access is safe here because this Bevy system is 
                    // guaranteed to run on the main thread and not concurrently with itself.
                    unsafe {
                        SAMPLE_ACCUMULATOR += 1.0;
                        if SAMPLE_ACCUMULATOR >= sample_step {
                            let sample = bus.apu.output();
                            for _ in 0..audio.channels {
                                let _ = audio.producer.push(sample);
                            }
                            SAMPLE_ACCUMULATOR -= sample_step;
                        }
                    }

                    for _ in 0..PPU_CPU_CYCLE_RATIO {
                        _frame_complete = bus.ppu.step();
                        if _frame_complete { break; }
                    }
                    if _frame_complete { break; }
                }
            }
            updated = true;
        }

        if updated {
            // Update the Bevy texture with the PPU's framebuffer
            for sprite in query.iter() {
                if let Some(image) = images.get_mut(&sprite.image) {
                    if let Some(data) = &mut image.data {
                        data.copy_from_slice(&emu.bus.ppu.frame_buffer);
                    }
                }
            }
        }
    }
}
