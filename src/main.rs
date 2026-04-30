mod core;

use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::window::PrimaryWindow;
use bevy_egui::{egui, EguiContext, EguiPlugin};
use egui_file_dialog::FileDialog;
use std::fs;

use std::io::Write;

use crate::core::bus::Bus;
use crate::core::cartridge::Cartridge;
use crate::core::cpu::Cpu;
use crate::core::joypad::JoypadButton;

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
            running: false,
        }
    }

    fn step_frame(&mut self) {
        if !self.running { return; }

        let mut _frame_complete = false;
        while !_frame_complete {
            let cycles = self.cpu.step(&mut self.bus);
            
            if self.bus.ppu.nmi_interrupt {
                self.cpu.nmi(&mut self.bus);
                self.bus.ppu.nmi_interrupt = false;
            }

            // PPU runs 3 times for every CPU cycle
            for _ in 0..(cycles * 3) {
                _frame_complete = self.bus.ppu.step();
                if _frame_complete { break; }
            }
        }

        static mut FRAMES: u32 = 0;
        unsafe {
            FRAMES += 1;
            if FRAMES == 120 {
                let mut f = std::fs::File::create("frame.raw").unwrap();
                f.write_all(&self.bus.ppu.frame_buffer).unwrap();
                std::fs::write("vram.bin", &self.bus.ppu.vram).unwrap();
                println!("PALETTE: {:?}", self.bus.ppu.palette_table);
                println!("DUMPED FRAME 120!");
            }
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
                resolution: (512, 480).into(),
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
        width: 256,
        height: 240,
        ..default()
    };
    
    // Create an empty image buffer filled with black
    let image = Image::new_fill(
        size,
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::MAIN_WORLD | bevy::asset::RenderAssetUsages::RENDER_WORLD,
    );
    
    // Add it to Bevy's asset system
    let image_handle = images.add(image);

    // Spawn a sprite to display the image
    commands.spawn((
        Sprite {
            image: image_handle,
            // custom_size: Some(Vec2::new(512.0, 480.0)),
            ..default()
        },
        Transform::from_scale(Vec3::new(2.0, 2.0, 1.0)), // Scale 2x for 512x480
        ScreenSprite,
    ));

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
        emu.bus.joypad1.set_button_pressed_status(JoypadButton::SELECT, keyboard_input.pressed(KeyCode::ShiftRight));

        // Run enough cycles to complete one frame
        emu.step_frame();

        // Update the Bevy texture with the PPU's framebuffer
        for sprite in query.iter() {
            if let Some(image) = images.get_mut(&sprite.image) {
                if let Some(data) = &mut image.data {
                    data.copy_from_slice(&emu.bus.ppu.frame_buffer);
                }
            }
        }
        
        // Debug: Dump frame to file occasionally
        static mut FRAME_COUNT: u32 = 0;
        unsafe {
            FRAME_COUNT += 1;
            if FRAME_COUNT == 120 {
                println!("DUMPED FRAME 120!");
                // (Optional: write to file)
            }
        }
    }
}
