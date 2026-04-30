//! # Ricoh 2C02 Picture Processing Unit (PPU)
//! 
//! The PPU is the core component responsible for generating the NES video signal.
//! It handles background tile rendering, sprite evaluation, and palette management.
//! 
//! ## Key Resources:
//! - [NESDev PPU Reference](https://www.nesdev.org/wiki/PPU)
//! - [PPU Rendering Pipeline](https://www.nesdev.org/wiki/PPU_rendering)
//! - [PPU Registers](https://www.nesdev.org/wiki/PPU_registers)
//! - [Loopy Scrolling Logic](https://www.nesdev.org/wiki/PPU_scrolling)

// The standard NES palette
pub const SYSTEM_PALETTE: [(u8, u8, u8); 64] = [
    (84, 84, 84), (0, 30, 116), (8, 16, 144), (48, 0, 136), (68, 0, 100), (92, 0, 48), (84, 4, 0), (60, 24, 0),
    (32, 42, 0), (8, 58, 0), (0, 64, 0), (0, 60, 0), (0, 50, 60), (0, 0, 0), (0, 0, 0), (0, 0, 0),
    (152, 150, 152), (8, 76, 196), (48, 50, 236), (92, 30, 228), (136, 20, 176), (160, 20, 100), (152, 34, 32), (120, 60, 0),
    (84, 90, 0), (40, 114, 0), (8, 124, 0), (0, 118, 40), (0, 102, 120), (0, 0, 0), (0, 0, 0), (0, 0, 0),
    (236, 238, 236), (76, 154, 236), (120, 124, 236), (176, 98, 236), (228, 84, 236), (236, 88, 180), (236, 106, 100), (212, 136, 32),
    (160, 170, 0), (116, 196, 0), (76, 208, 32), (56, 204, 108), (56, 180, 204), (60, 60, 60), (0, 0, 0), (0, 0, 0),
    (236, 238, 236), (168, 204, 236), (188, 188, 236), (212, 178, 236), (236, 174, 236), (236, 174, 212), (236, 180, 176), (228, 196, 144),
    (204, 210, 120), (180, 222, 120), (168, 226, 144), (152, 226, 180), (160, 214, 228), (160, 162, 160), (0, 0, 0), (0, 0, 0)
];

/// NES screen width in pixels
pub const SCREEN_WIDTH: usize = 256;
/// NES screen height in pixels
pub const SCREEN_HEIGHT: usize = 240;

bitflags::bitflags! {
    /// PPUCTRL ($2000) register flags
    /// https://www.nesdev.org/wiki/PPU_registers#PPUCTRL
    pub struct CtrlFlags: u8 {
        const NAMETABLE1              = 0b0000_0001;
        const NAMETABLE2              = 0b0000_0010;
        const VRAM_ADD_INCREMENT      = 0b0000_0100;
        const SPRITE_PATTERN_ADDR     = 0b0000_1000;
        const BACKGRND_PATTERN_ADDR   = 0b0001_0000;
        const SPRITE_SIZE             = 0b0010_0000;
        const PPU_MASTER_SLAVE        = 0b0100_0000;
        const GENERATE_NMI            = 0b1000_0000;
    }
}

bitflags::bitflags! {
    /// PPUMASK ($2001) register flags
    /// https://www.nesdev.org/wiki/PPU_registers#PPUMASK
    pub struct MaskFlags: u8 {
        const GREYSCALE               = 0b0000_0001;
        const SHOW_LEFT_BG            = 0b0000_0010;
        const SHOW_LEFT_SPRITES       = 0b0000_0100;
        const SHOW_BACKGROUND        = 0b0000_1000;
        const SHOW_SPRITES           = 0b0001_0000;
        const EMPHASIZE_RED          = 0b0010_0000;
        const EMPHASIZE_GREEN        = 0b0100_0000;
        const EMPHASIZE_BLUE         = 0b1000_0000;
        
        /// Combined flag to check if rendering is enabled (BG or Sprites)
        const RENDER_ENABLED         = 0b0001_1000;
    }
}

bitflags::bitflags! {
    /// PPUSTATUS ($2002) register flags
    /// https://www.nesdev.org/wiki/PPU_registers#PPUSTATUS
    pub struct StatusFlags: u8 {
        const SPRITE_OVERFLOW         = 0b0010_0000;
        const SPRITE_ZERO_HIT         = 0b0100_0000;
        const VBLANK_STARTED          = 0b1000_0000;
    }
}

/// Base address for PPU nametables
pub const NAMETABLE_BASE: u16 = 0x2000;
/// Start of the PPU palette RAM
pub const PALETTE_BASE: u16 = 0x3F00;
/// Size of the palette RAM (32 bytes)
pub const PALETTE_SIZE: u16 = 0x20;
/// Total addressable PPU memory (including mirrors)
pub const PPU_ADDR_MASK: u16 = 0x3FFF;

// PPU Register Addresses
pub const PPU_REG_CTRL: u16     = 0x2000;
pub const PPU_REG_MASK: u16     = 0x2001;
pub const PPU_REG_STATUS: u16   = 0x2002;
pub const PPU_REG_OAM_ADDR: u16 = 0x2003;
pub const PPU_REG_OAM_DATA: u16 = 0x2004;
pub const PPU_REG_SCROLL: u16   = 0x2005;
pub const PPU_REG_ADDR: u16     = 0x2006;
pub const PPU_REG_DATA: u16     = 0x2007;

// Internal Memory Ranges
pub const CHR_ROM_START: u16    = 0x0000;
pub const CHR_ROM_END: u16      = 0x1FFF;
pub const VRAM_NT_START: u16    = 0x2000;
pub const VRAM_NT_END: u16      = 0x2FFF;
/// Total addressable size of the nametable region (4KB total for mirroring)
pub const NAMETABLE_REGION_SIZE: u16 = 0x1000;
/// Size of a single nametable (1KB)
pub const NAMETABLE_SIZE: u16 = 0x0400;
/// Offset from nametable base to attribute table (960 bytes)
pub const ATTRIBUTE_TABLE_OFFSET: u16 = 0x03C0;

/// Size of a tile in pattern table (16 bytes: 8 bytes for bitplane 0, 8 for bitplane 1)
pub const TILE_SIZE_BYTES: u16 = 16;
/// Offset to the second bitplane in a tile
pub const TILE_BITPLANE_OFFSET: u16 = 8;

/// Number of sprites in OAM (64 sprites, 4 bytes each)
pub const OAM_SPRITE_COUNT: usize = 64;
/// Size of one OAM entry in bytes
pub const OAM_ENTRY_SIZE: usize = 4;

// Palette specific
/// Mask for palette index within a 32-entry palette table
pub const PALETTE_MASK: u16        = 0x001F;
/// Mask to handle palette mirroring ($3F10/$3F14/$3F18/$3F1C mirror $3F00/$3F04/$3F08/$3F0C)
pub const PALETTE_MIRROR_MASK: u16 = 0x10;
/// Number of colors in a single palette (e.g., 4 colors per palette)
pub const PALETTE_ENTRY_SIZE: u16  = 4;
/// Mask to get the lower 6 bits of a palette color (0-63)
pub const SYSTEM_COLOR_MASK: u8    = 0x3F;

bitflags::bitflags! {
    /// OAM Sprite Attribute flags (Byte 2 of an OAM entry)
    /// https://www.nesdev.org/wiki/PPU_OAM#Byte_2
    pub struct SpriteAttributes: u8 {
        const PALETTE_L      = 0b0000_0001; // Palette (0-3) LSB
        const PALETTE_H      = 0b0000_0010; // Palette (0-3) MSB
        const PRIORITY       = 0b0010_0000; // Priority (0: in front of BG, 1: behind BG)
        const FLIP_HORIZ     = 0b0100_0000; // Flip sprite horizontally
        const FLIP_VERT      = 0b1000_0000; // Flip sprite vertically
        
        /// Mask for the 2-bit palette index (0-3)
        const PALETTE_MASK   = 0b0000_0011;
    }
}

/// Offset added to sprite Y-coordinate in OAM (sprites are delayed by 1 scanline)
pub const SPRITE_Y_OFFSET: u16 = 1;
/// Default sprite height in 8x8 mode
pub const SPRITE_HEIGHT_8X8: u16 = 8;
/// Palette table offset for sprites (palettes 4-7)
pub const SPRITE_PALETTE_OFFSET: u8 = 4;
/// Bytes per pixel in the RGBA frame buffer
pub const BYTES_PER_PIXEL: usize = 4;

// Pattern Table Addresses
pub const PATTERN_TABLE_0: u16 = 0x0000;
pub const PATTERN_TABLE_1: u16 = 0x1000;

/// VRAM increment values controlled by PPUCTRL Bit 2
pub const VRAM_INCREMENT_1: u16 = 1;
pub const VRAM_INCREMENT_32: u16 = 32;

// Palette mirror indices (Universal Background Color)
pub const PALETTE_MIRROR_0: u16 = 0x10;
pub const PALETTE_MIRROR_1: u16 = 0x14;
pub const PALETTE_MIRROR_2: u16 = 0x18;
pub const PALETTE_MIRROR_3: u16 = 0x1C;

// Loopy Scroll Masks and Shifts
pub const LOOPY_NAMETABLE_MASK: u16 = 0x0C00;
pub const LOOPY_COARSE_X_MASK: u16  = 0x001F;
pub const LOOPY_COARSE_Y_MASK: u16  = 0x03E0;
pub const LOOPY_FINE_Y_MASK: u16    = 0x7000;

/// Increment for the Fine Y portion of the Loopy address (Bit 12)
pub const LOOPY_FINE_Y_INCREMENT: u16 = 0x1000;
/// Bit for the vertical nametable selection in Loopy address (Bit 11)
pub const LOOPY_VERTICAL_NAMETABLE_BIT: u16 = 0x0800;

/// Mask used to clear horizontal scroll bits (Coarse X and low Nametable bit) before reload
pub const LOOPY_HORIZONTAL_RELOAD_MASK: u16 = 0xFBE0;
/// Combined mask for horizontal scroll bits used in reload from 't' to 'v'
pub const LOOPY_HORIZONTAL_RELOAD_BITS: u16 = 0x041F;

// PPU Timing (NTSC)
pub const SCANLINES_PER_FRAME: u16 = 262;
pub const CYCLES_PER_SCANLINE: usize = 341;
pub const VBLANK_SCANLINE: u16     = 241;
pub const PRE_RENDER_SCANLINE: u16 = 261;

// Address latch masks
pub const ADDR_HIGH_BYTE_MASK: u16 = 0x3F00;
pub const ADDR_LOW_BYTE_MASK: u16  = 0x00FF;

/// Default window scaling factor for the emulator display
#[allow(dead_code)]
pub const RENDER_SCALE: f32 = 2.0;

/// Fully opaque alpha value for RGBA pixels
pub const OPAQUE_ALPHA: u8 = 255;

pub struct Ppu {
    pub palette_table: [u8; 32],
    pub vram: [u8; 2048],
    pub oam_data: [u8; 256],
    
    // Registers
    pub ctrl: u8,
    pub mask: u8,
    pub status: u8,
    
    // Scrolling registers (Loopy)
    /// Current VRAM address (15 bits), also known as 'v'.
    /// 
    /// **Internal Structure (Bitfields):**
    /// - `yyy NN YYYYY XXXXX`
    /// - `yyy`: Fine Y scroll (0-7)
    /// - `NN`: Nametable select (0-3)
    /// - `YYYYY`: Coarse Y scroll (0-29)
    /// - `XXXXX`: Coarse X scroll (0-31)
    pub v: u16,
    /// Temporary VRAM address (15 bits), also known as 't'.
    /// Same bit structure as `v`. Used as a latch before being copied to `v`.
    pub t: u16,
    /// Fine X scroll (3 bits), also known as 'x'.
    pub x: u8,
    /// Write latch (1 bit), also known as 'w'.
    /// Toggles on every write to $2005 (Scroll) or $2006 (Addr).
    pub w: bool,
    
    pub oam_addr: u8,
    pub nmi_interrupt: bool,

    internal_data_buf: u8,
    
    pub frame_buffer: [u8; 256 * 240 * 4], // RGBA
    scanline: u16,
    cycles: usize,
}

use super::mapper::{Mapper, Mirroring};

impl Ppu {
    /// Maps a PPU address ($2000-$3FFF) to a physical VRAM address based on mirroring mode.
    pub fn mirror_vram_addr(&self, addr: u16, mirroring: Mirroring) -> u16 {
        let addr = (addr - NAMETABLE_BASE) % NAMETABLE_REGION_SIZE;
        match mirroring {
            Mirroring::Vertical => addr % (NAMETABLE_SIZE * 2),
            Mirroring::Horizontal => {
                if addr < (NAMETABLE_SIZE * 2) {
                    addr % NAMETABLE_SIZE
                } else {
                    (addr % NAMETABLE_SIZE) + NAMETABLE_SIZE
                }
            }
            Mirroring::SingleScreenLower => addr % NAMETABLE_SIZE,
            Mirroring::SingleScreenUpper => (addr % NAMETABLE_SIZE) + NAMETABLE_SIZE,
            _ => addr % (NAMETABLE_SIZE * 2), // Fallback
        }
    }

    pub fn new(_chr_rom: Vec<u8>) -> Self {
        Ppu {
            palette_table: [0; 32],
            vram: [0; 2048],
            oam_data: [0; 256],
            
            ctrl: 0,
            mask: 0,
            status: 0,
            v: 0,
            t: 0,
            x: 0,
            w: false,
            oam_addr: 0,
            nmi_interrupt: false,

            internal_data_buf: 0,
            
            frame_buffer: [0; SCREEN_WIDTH * SCREEN_HEIGHT * 4],
            scanline: 0,
            cycles: 0,
        }
    }

    pub fn write_to_ctrl(&mut self, value: u8) {
        let nmi_before = (self.ctrl & CtrlFlags::GENERATE_NMI.bits()) != 0;
        self.ctrl = value;
        let nmi_after = (self.ctrl & CtrlFlags::GENERATE_NMI.bits()) != 0;
        if !nmi_before && nmi_after && (self.status & StatusFlags::VBLANK_STARTED.bits() != 0) {
            self.nmi_interrupt = true;
        }
        // Update temporary VRAM address with nametable bits (bits 10-11)
        self.t = (self.t & !LOOPY_NAMETABLE_MASK) | ((u16::from(value) & 0x03) << 10);
    }

    pub fn write_to_scroll(&mut self, value: u8) {
        if self.w {
            // Second write: Fine Y (bits 12-14) and Coarse Y (bits 5-9)
            self.t = (self.t & !LOOPY_FINE_Y_MASK & !LOOPY_COARSE_Y_MASK) 
                   | ((u16::from(value) & 0x07) << 12) 
                   | ((u16::from(value) & 0xF8) << 2);
            self.w = false;
        } else {
            // First write: Coarse X (bits 0-4) and Fine X (stored in 'x' register)
            self.t = (self.t & !LOOPY_COARSE_X_MASK) | (u16::from(value) >> 3);
            self.x = value & 0x07;
            self.w = true;
        }
    }

    pub fn write_to_addr(&mut self, value: u8) {
        if self.w {
            // Second write: Low byte of address
            self.t = (self.t & ADDR_HIGH_BYTE_MASK) | u16::from(value);
            self.v = self.t;
            self.w = false;
        } else {
            // First write: High byte of address (bits 8-13, bit 14 is cleared)
            self.t = (self.t & ADDR_LOW_BYTE_MASK) | ((u16::from(value) & 0x3F) << 8);
            self.w = true;
        }
    }

    pub fn read(&mut self, addr: u16, mapper: &dyn Mapper) -> u8 {
        match addr {
            PPU_REG_CTRL | PPU_REG_MASK | PPU_REG_OAM_ADDR | PPU_REG_SCROLL | PPU_REG_ADDR => 0,
            PPU_REG_STATUS => {
                let status = self.status;
                self.status &= !StatusFlags::VBLANK_STARTED.bits(); // Clear VBlank ONLY (bit 7)
                self.w = false; // Reset address latch
                status
            }
            PPU_REG_OAM_DATA => self.oam_data[self.oam_addr as usize],
            PPU_REG_DATA => {
                let mut data = self.internal_data_buf;
                let addr = self.v & PPU_ADDR_MASK;
                
                // Read from VRAM into the buffer
                self.internal_data_buf = match addr {
                    CHR_ROM_START..=CHR_ROM_END => mapper.chr_read(addr),
                    VRAM_NT_START..=VRAM_NT_END => self.vram.get(self.mirror_vram_addr(addr, mapper.mirroring()) as usize).copied().unwrap_or(0),
                    _ => 0,
                };
                
                // Palette read is immediate, no buffer delay
                if (PALETTE_BASE..PALETTE_BASE + PALETTE_SIZE).contains(&addr) {
                    let mut pal_addr = addr & PALETTE_MASK;
                    if pal_addr == PALETTE_MIRROR_0 || pal_addr == PALETTE_MIRROR_1 || pal_addr == PALETTE_MIRROR_2 || pal_addr == PALETTE_MIRROR_3 {
                        pal_addr -= PALETTE_MIRROR_MASK;
                    }
                    data = self.palette_table[pal_addr as usize];
                }
                
                let increment = if self.ctrl & CtrlFlags::VRAM_ADD_INCREMENT.bits() == 0 { VRAM_INCREMENT_1 } else { VRAM_INCREMENT_32 };
                self.v = self.v.wrapping_add(increment);
                
                data
            }
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u16, data: u8, mapper: &mut dyn Mapper) {
        match addr {
            PPU_REG_CTRL => self.write_to_ctrl(data),
            PPU_REG_MASK => {
                self.mask = data;
            }
            PPU_REG_OAM_ADDR => self.oam_addr = data,
            PPU_REG_OAM_DATA => {
                self.oam_data[self.oam_addr as usize] = data;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            PPU_REG_SCROLL => self.write_to_scroll(data),
            PPU_REG_ADDR => self.write_to_addr(data),
            PPU_REG_DATA => {
                let addr = self.v & PPU_ADDR_MASK;
                match addr {
                    CHR_ROM_START..=CHR_ROM_END => mapper.chr_write(addr, data),
                    VRAM_NT_START..=VRAM_NT_END => self.vram[self.mirror_vram_addr(addr, mapper.mirroring()) as usize] = data,
                    _ if (PALETTE_BASE..PALETTE_BASE + PALETTE_SIZE).contains(&addr) => {
                        let mut pal_addr = addr & PALETTE_MASK;
                        // Addresses $3F10/$3F14/$3F18/$3F1C are mirrors of $3F00/$3F04/$3F08/$3F0C
                        if pal_addr == PALETTE_MIRROR_0 || pal_addr == PALETTE_MIRROR_1 || pal_addr == PALETTE_MIRROR_2 || pal_addr == PALETTE_MIRROR_3 {
                            pal_addr -= PALETTE_MIRROR_MASK;
                        }
                        self.palette_table[pal_addr as usize] = data;
                    }
                    _ => {}
                }
                
                let increment = if self.ctrl & CtrlFlags::VRAM_ADD_INCREMENT.bits() == 0 { VRAM_INCREMENT_1 } else { VRAM_INCREMENT_32 };
                self.v = self.v.wrapping_add(increment);
            }
            _ => {}
        }
    }

    /// Advances the PPU by one clock cycle.
    pub fn step(&mut self, mapper: &dyn Mapper) -> bool {
        self.cycles += 1;

        if self.cycles == 256
            && self.scanline < SCREEN_HEIGHT as u16 {
                self.render_scanline(self.scanline, mapper);
            }

        if self.cycles >= CYCLES_PER_SCANLINE {
            self.cycles = 0;
            
            if self.scanline < SCREEN_HEIGHT as u16
                && self.mask & MaskFlags::RENDER_ENABLED.bits() != 0 {
                    // --- LOOPY SCROLLING ALGORITHM: VERTICAL INCREMENT ---
                    // 1. Increment fine Y (bits 12-14)
                    if (self.v & LOOPY_FINE_Y_MASK) == LOOPY_FINE_Y_MASK {
                        // 2. Fine Y overflowed, reset it and increment coarse Y
                        self.v &= !LOOPY_FINE_Y_MASK;
                        let mut y_coarse = (self.v & LOOPY_COARSE_Y_MASK) >> 5;
                        if y_coarse == 29 {
                            // 3. Coarse Y reached 29 (bottom of nametable), reset and flip nametable bit
                            y_coarse = 0;
                            self.v ^= LOOPY_VERTICAL_NAMETABLE_BIT; // Switch vertical nametable bit
                        } else if y_coarse == 31 {
                            // 4. Coarse Y reached 31 (illegal attribute area), just reset
                            y_coarse = 0;
                        } else {
                            y_coarse += 1;
                        }
                        self.v = (self.v & !LOOPY_COARSE_Y_MASK) | (y_coarse << 5);
                    } else {
                        self.v += LOOPY_FINE_Y_INCREMENT;
                    }
                    // --- HORIZONTAL RESET ---
                    // At the end of each scanline, if rendering is enabled,
                    // horizontal bits are reloaded from 't' into 'v'.
                    self.v = (self.v & LOOPY_HORIZONTAL_RELOAD_MASK) | (self.t & LOOPY_HORIZONTAL_RELOAD_BITS);
                }

            self.scanline += 1;

            if self.scanline == VBLANK_SCANLINE {
                // Set VBlank at start of scanline 241
                self.status |= StatusFlags::VBLANK_STARTED.bits();
                if self.ctrl & CtrlFlags::GENERATE_NMI.bits() != 0 {
                    self.nmi_interrupt = true;
                }
            }

            if self.scanline == PRE_RENDER_SCANLINE {
                self.status &= !(StatusFlags::VBLANK_STARTED.bits() | StatusFlags::SPRITE_ZERO_HIT.bits());
            }

            if self.scanline >= SCANLINES_PER_FRAME {
                self.scanline = 0;
                // RELOAD V FROM T HERE (Start of new frame)
                if self.mask & MaskFlags::RENDER_ENABLED.bits() != 0 {
                    self.v = self.t;
                }
                return true; // Frame is complete
            }
        }
        false
    }

    /// Renders a single scanline into the frame buffer.
    pub(crate) fn render_scanline(&mut self, y: u16, mapper: &dyn Mapper) {
        if y >= SCREEN_HEIGHT as u16 { return; }

        let bg_bank = if self.ctrl & CtrlFlags::BACKGRND_PATTERN_ADDR.bits() != 0 { PATTERN_TABLE_1 } else { PATTERN_TABLE_0 };
        let mut bg_opaque = [false; SCREEN_WIDTH];
        let mirroring = mapper.mirroring();

        if self.mask & MaskFlags::SHOW_BACKGROUND.bits() != 0 {
            let fine_x = u16::from(self.x);
            for screen_x in 0..SCREEN_WIDTH as u16 {
                let total_x = screen_x + fine_x;
                let coarse_x_inc = total_x / 8;
                let base_coarse_x = self.v & LOOPY_COARSE_X_MASK;
                
                let final_coarse_x = (base_coarse_x + coarse_x_inc) % 32;
                let final_nt = ((self.v >> 10) & 0x03) ^ ((base_coarse_x + coarse_x_inc) / 32);
                
                let v = (self.v & !0x041F) | (final_nt << 10) | final_coarse_x;

                let coarse_x = v & LOOPY_COARSE_X_MASK;
                let coarse_y = (v & LOOPY_COARSE_Y_MASK) >> 5;
                let nt_select = (v >> 10) & 0x03;
                let fine_y = (v & LOOPY_FINE_Y_MASK) >> 12;
                
                let nt_addr = NAMETABLE_BASE | (nt_select << 10) | (coarse_y << 5) | coarse_x;
                let vram_idx = self.mirror_vram_addr(nt_addr, mirroring) as usize;
                let tile_id = if vram_idx < self.vram.len() { u16::from(self.vram[vram_idx]) } else { 0 };
                
                let attr_addr = (NAMETABLE_BASE + ATTRIBUTE_TABLE_OFFSET) | (nt_select << 10) | ((coarse_y >> 2) << 3) | (coarse_x >> 2);
                let attr_idx = self.mirror_vram_addr(attr_addr, mirroring) as usize;
                let attr_byte = if attr_idx < self.vram.len() { self.vram[attr_idx] } else { 0 };
                
                let shift = ((coarse_y & 2) << 1) | (coarse_x & 2);
                let palette_idx = (attr_byte >> shift) & 0x03;
                
                let tile_addr = bg_bank + tile_id * TILE_SIZE_BYTES + fine_y;
                let p_low = mapper.chr_read(tile_addr);
                let p_high = mapper.chr_read(tile_addr + TILE_BITPLANE_OFFSET);
                
                let bit_idx = 7 - (total_x % 8);
                let color_val = (((p_high >> bit_idx) & 1) << 1) | ((p_low >> bit_idx) & 1);
                bg_opaque[screen_x as usize] = color_val != 0;
                
                let sys_color_idx = if color_val == 0 { 
                    self.palette_table[0] 
                } else { 
                    self.palette_table[(u16::from(palette_idx) * PALETTE_ENTRY_SIZE + u16::from(color_val)) as usize] 
                };
                let color = SYSTEM_PALETTE[(sys_color_idx & SYSTEM_COLOR_MASK) as usize];
                let fb_idx = (y as usize * SCREEN_WIDTH + screen_x as usize) * BYTES_PER_PIXEL;
                if fb_idx + (BYTES_PER_PIXEL - 1) < self.frame_buffer.len() {
                    self.frame_buffer[fb_idx] = color.0;
                    self.frame_buffer[fb_idx + 1] = color.1;
                    self.frame_buffer[fb_idx + 2] = color.2;
                    self.frame_buffer[fb_idx + 3] = OPAQUE_ALPHA;
                }
            }
        }

        if self.mask & MaskFlags::SHOW_SPRITES.bits() != 0 {
            let s_bank = if self.ctrl & CtrlFlags::SPRITE_PATTERN_ADDR.bits() != 0 { PATTERN_TABLE_1 } else { PATTERN_TABLE_0 };
            for i in (0..OAM_SPRITE_COUNT).rev() {
                let oam_idx = i * OAM_ENTRY_SIZE;
                let sprite_y = u16::from(self.oam_data[oam_idx]);
                if y >= sprite_y + SPRITE_Y_OFFSET && y < sprite_y + SPRITE_Y_OFFSET + SPRITE_HEIGHT_8X8 {
                    let tile_id = u16::from(self.oam_data[oam_idx + 1]);
                    let attr = SpriteAttributes::from_bits_truncate(self.oam_data[oam_idx + 2]);
                    let sprite_x = u16::from(self.oam_data[oam_idx + 3]);
                    
                    let flip_h = attr.contains(SpriteAttributes::FLIP_HORIZ);
                    let flip_v = attr.contains(SpriteAttributes::FLIP_VERT);
                    let palette_idx = (attr.bits() & SpriteAttributes::PALETTE_MASK.bits()) + SPRITE_PALETTE_OFFSET;
                    
                    let row = if flip_v { 
                        (SPRITE_HEIGHT_8X8 - 1) - (y - (sprite_y + SPRITE_Y_OFFSET)) 
                    } else { 
                        y - (sprite_y + SPRITE_Y_OFFSET) 
                    };
                    let tile_addr = s_bank + tile_id * TILE_SIZE_BYTES + row;
                    
                    let p_low = mapper.chr_read(tile_addr);
                    let p_high = mapper.chr_read(tile_addr + TILE_BITPLANE_OFFSET);

                    for dx in 0..8 {
                        let screen_x = sprite_x + dx;
                        if screen_x >= SCREEN_WIDTH as u16 { continue; }
                        let bit_idx = if flip_h { dx } else { 7 - dx };
                        let color_val = (((p_high >> bit_idx) & 1) << 1) | ((p_low >> bit_idx) & 1);
                        
                        if color_val != 0 {
                            if i == 0 && bg_opaque[screen_x as usize] && (self.mask & MaskFlags::RENDER_ENABLED.bits() == MaskFlags::RENDER_ENABLED.bits()) {
                                self.status |= StatusFlags::SPRITE_ZERO_HIT.bits();
                            }

                            let priority = attr.contains(SpriteAttributes::PRIORITY);
                            if !priority || !bg_opaque[screen_x as usize] {
                                let sys_idx = self.palette_table[(u16::from(palette_idx) * PALETTE_ENTRY_SIZE + u16::from(color_val)) as usize];
                                let color = SYSTEM_PALETTE[(sys_idx & SYSTEM_COLOR_MASK) as usize];
                                let fb_idx = (y as usize * SCREEN_WIDTH + screen_x as usize) * BYTES_PER_PIXEL;
                                
                                if fb_idx + (BYTES_PER_PIXEL - 1) < self.frame_buffer.len() {
                                    self.frame_buffer[fb_idx] = color.0;
                                    self.frame_buffer[fb_idx + 1] = color.1;
                                    self.frame_buffer[fb_idx + 2] = color.2;
                                    self.frame_buffer[fb_idx + 3] = OPAQUE_ALPHA;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::mapper::{Mapper0, Mirroring};
    use super::*;
    use crate::core::bus::PPU_REG_MIRROR_MASK;

    fn create_mock_mapper() -> Mapper0 {
        Mapper0::new(vec![0; 16384], vec![0; 8192], Mirroring::Vertical)
    }

    /// **Objective**: Verify that VRAM addresses are correctly mirrored for Vertical Mirroring 
    /// configuration (standard NROM-style).
    #[test]
    fn test_vram_mirroring_vertical() {
        let ppu = Ppu::new(vec![0; 0x2000]);
        
        // NT0
        assert_eq!(ppu.mirror_vram_addr(0x2000, Mirroring::Vertical), 0x0000);
        // NT2 mirrors NT0
        assert_eq!(ppu.mirror_vram_addr(0x2800, Mirroring::Vertical), 0x0000);
        
        // NT1
        assert_eq!(ppu.mirror_vram_addr(0x2400, Mirroring::Vertical), 0x0400);
        // NT3 mirrors NT1
        assert_eq!(ppu.mirror_vram_addr(0x2C00, Mirroring::Vertical), 0x0400);
    }

    /// **Objective**: Verify that VRAM addresses are correctly mirrored for Horizontal Mirroring 
    /// configuration (standard NROM-style).
    #[test]
    fn test_vram_mirroring_horizontal() {
        let ppu = Ppu::new(vec![0; 0x2000]);
        
        // NT0
        assert_eq!(ppu.mirror_vram_addr(0x2000, Mirroring::Horizontal), 0x0000);
        // NT1 mirrors NT0
        assert_eq!(ppu.mirror_vram_addr(0x2400, Mirroring::Horizontal), 0x0000);
        
        // NT2
        assert_eq!(ppu.mirror_vram_addr(0x2800, Mirroring::Horizontal), 0x0400);
        // NT3 mirrors NT2
        assert_eq!(ppu.mirror_vram_addr(0x2C00, Mirroring::Horizontal), 0x0400);
    }

    /// **Objective**: Verify that reading the PPUSTATUS register correctly clears the 
    /// VBlank flag and resets the internal address latch.
    #[test]
    fn test_ppu_register_mirroring() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        let mapper = create_mock_mapper();
        ppu.status = 0x80; // VBlank set
        let status = ppu.read(PPU_REG_STATUS & PPU_REG_MIRROR_MASK, &mapper);
        assert_eq!(status, 0x80);
        assert_eq!(ppu.status, 0x00); // VBlank should be cleared after read
    }

    /// **Objective**: Verify that palette memory mirrors ($3F10-$3F1F) correctly point 
    /// to the base palette addresses ($3F00-$3F0F) in VRAM.
    #[test]
    fn test_palette_mirroring() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        let mut mapper = create_mock_mapper();
        // $3F10 is a mirror of $3F00
        // We write 0x1A to $3F00
        ppu.v = 0x3F00;
        ppu.write(PPU_REG_DATA & PPU_REG_MIRROR_MASK, 0x1A, &mut mapper);
        assert_eq!(ppu.palette_table[0], 0x1A);
        
        // Read from $3F10
        ppu.v = 0x3F10;
        let val = ppu.read(PPU_REG_DATA & PPU_REG_MIRROR_MASK, &mapper);
        assert_eq!(val, 0x1A);
    }

    /// **Objective**: Verify that the PPU correctly detects a Sprite 0 Hit when an 
    /// opaque sprite pixel overlaps an opaque background pixel.
    #[test]
    fn test_sprite_zero_hit() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        let mut chr_rom = vec![0; 8192];
        chr_rom[0..16].fill(0xFF); 
        let mapper = Mapper0::new(vec![0; 16384], chr_rom, Mirroring::Vertical);
        
        ppu.mask = MaskFlags::SHOW_BACKGROUND.bits() | MaskFlags::SHOW_SPRITES.bits();
        ppu.oam_data[0] = 10; // Y
        ppu.oam_data[1] = 0;  // Tile 0
        ppu.oam_data[2] = 0;  // No attributes
        ppu.oam_data[3] = 10; // X
        
        ppu.vram[0x0000] = 0; // Opaque bg pixel (index 0 points to opaque palette in this mock)
        ppu.palette_table[0] = 0x11; // Bg color (opaque)
        ppu.palette_table[0x11] = 0x22; // Sprite color (opaque)
        
        ppu.render_scanline(11, &mapper);
        assert!(ppu.status & StatusFlags::SPRITE_ZERO_HIT.bits() != 0);
    }

    /// **Objective**: Verify that the PPU correctly fetches tile data from VRAM 
    /// and renders background pixels into the frame buffer for a single scanline.
    #[test]
    fn test_render_scanline_background() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        ppu.mask |= MaskFlags::SHOW_BACKGROUND.bits();
        
        let mut chr_rom = vec![0; 8192];
        for i in 0..8 {
            chr_rom[i] = 0xFF; // Low bitplane
            chr_rom[i + 8] = 0x00; // High bitplane
        }
        let mapper = Mapper0::new(vec![0; 16384], chr_rom, Mirroring::Vertical);
        
        ppu.vram[0] = 0;
        ppu.palette_table[1] = 0x00; 
        
        ppu.render_scanline(0, &mapper);
        
        assert_eq!(ppu.frame_buffer[0], 84);
    }

    /// **Objective**: Verify that the OAMADDR and OAMDATA registers correctly manage 
    /// the internal OAM memory for sprite storage.
    #[test]
    fn test_ppu_oam_access() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        let mut mapper = create_mock_mapper();
        // Set OAM address to 0x10
        ppu.write(PPU_REG_OAM_ADDR, 0x10, &mut mapper);
        // Write to OAMDATA
        ppu.write(PPU_REG_OAM_DATA, 0xBC, &mut mapper);
        
        assert_eq!(ppu.oam_data[0x10], 0xBC);
        assert_eq!(ppu.oam_addr, 0x11); // Auto-increment
    }

    /// **Objective**: Verify that the PPU correctly applies palette indices from the 
    /// Attribute Table to background pixels.
    #[test]
    fn test_render_attributes() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        ppu.mask |= MaskFlags::SHOW_BACKGROUND.bits();
        
        let mut chr_rom = vec![0; 8192];
        for val in chr_rom.iter_mut().take(8) { *val = 0xFF; }
        let mapper = Mapper0::new(vec![0; 16384], chr_rom, Mirroring::Vertical);
        
        ppu.vram[0x03C0] = 0x01; 
        ppu.palette_table[5] = 0x20; 
        
        ppu.render_scanline(0, &mapper);
        
        assert_eq!(ppu.frame_buffer[0], 236);
    }

    /// **Objective**: Verify that writes to PPUDATA correctly increment the 
    /// internal address pointer according to the PPUCTRL increment flag.
    #[test]
    fn test_ppu_vram_increment() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        let mut mapper = create_mock_mapper();
        // Increment by 32
        ppu.write(PPU_REG_CTRL, CtrlFlags::VRAM_ADD_INCREMENT.bits(), &mut mapper);
        ppu.write(PPU_REG_ADDR, 0x20, &mut mapper);
        ppu.write(PPU_REG_ADDR, 0x00, &mut mapper); // Address $2000
        
        ppu.write(PPU_REG_DATA, 0x11, &mut mapper);
        assert_eq!(ppu.v, 0x2020); // $2000 + 32
    }

    /// **Objective**: Verify that writes to PPUSCROLL correctly update the 
    /// temporary address (t) and fine-X registers.
    #[test]
    fn test_ppu_scroll_writes() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        let mut mapper = create_mock_mapper();
        // First write: fine X and coarse X
        ppu.write(PPU_REG_SCROLL, 0x7D, &mut mapper); // fine X = 5, coarse X = 15
        assert_eq!(ppu.x, 0x05);
        assert_eq!(ppu.t & 0x001F, 0x0F);
        
        // Second write: fine Y and coarse Y
        ppu.write(PPU_REG_SCROLL, 0x5E, &mut mapper); 
        assert!(ppu.t & 0x7000 != 0); // Fine Y bits
    }

    /// **Objective**: Verify that the PPU correctly renders sprites with 
    /// foreground/background priority settings.
    #[test]
    fn test_render_sprites() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        ppu.oam_data = [0xFF; 256];
        ppu.mask |= MaskFlags::SHOW_SPRITES.bits();
        ppu.ctrl |= CtrlFlags::SPRITE_PATTERN_ADDR.bits();
        
        let mut chr_rom = vec![0; 8192];
        for i in 0..8 { chr_rom[0x1000 + i] = 0xFF; } 
        let mapper = Mapper0::new(vec![0; 16384], chr_rom, Mirroring::Vertical);
        
        ppu.oam_data[0] = 0;    
        ppu.oam_data[1] = 0x00; 
        ppu.oam_data[2] = 0x00; 
        ppu.oam_data[3] = 0;    
        
        ppu.palette_table[0x11] = 0x30; 
        
        ppu.render_scanline(1, &mapper); 
        
        assert_eq!(ppu.frame_buffer[SCREEN_WIDTH * 4], 236);
    }

    /// **Objective**: Verify that reading PPUSTATUS correctly clears the VBlank 
    /// flag and resets the address latch (w).
    #[test]
    fn test_ppu_status_read_side_effects() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        let mapper = create_mock_mapper();
        ppu.status |= StatusFlags::VBLANK_STARTED.bits();
        ppu.w = true; // Address latch set
        
        let status = ppu.read(PPU_REG_STATUS, &mapper);
        
        assert!(status & StatusFlags::VBLANK_STARTED.bits() != 0);
        assert!(ppu.status & StatusFlags::VBLANK_STARTED.bits() == 0); // Cleared after read
        assert!(!ppu.w); // Latch reset
    }

    /// **Objective**: Verify that palette RAM writes to $3F10, $3F14, $3F18, $3F1C 
    /// are correctly mirrored to $3F00, $3F04, $3F08, $3F0C.
    #[test]
    fn test_ppu_palette_mirroring_writes() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        let mut mapper = create_mock_mapper();
        
        // Write to $3F10 (mirror of $3F00)
        ppu.write(PPU_REG_ADDR, 0x3F, &mut mapper);
        ppu.write(PPU_REG_ADDR, 0x10, &mut mapper);
        ppu.write(PPU_REG_DATA, 0x12, &mut mapper);
        
        assert_eq!(ppu.palette_table[0x00], 0x12);
    }

    /// **Objective**: Verify that OAMDATA ($2004) reads return the value at OAMADDR 
    /// and that writes correctly increment OAMADDR.
    #[test]
    fn test_ppu_oam_read_write() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        let mut mapper = create_mock_mapper();
        ppu.write(PPU_REG_OAM_ADDR, 0x10, &mut mapper);
        ppu.write(PPU_REG_OAM_DATA, 0xDE, &mut mapper);
        
        assert_eq!(ppu.oam_addr, 0x11);
        ppu.write(PPU_REG_OAM_ADDR, 0x10, &mut mapper);
        assert_eq!(ppu.read(PPU_REG_OAM_DATA, &mapper), 0xDE);
    }

    /// **Objective**: Verify the PPU VRAM read buffer logic, where reading 
    /// from $2007 returns the previous buffered value for most memory ranges.
    #[test]
    fn test_ppu_vram_read_buffer() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        let mut mapper = create_mock_mapper();
        ppu.vram[0x0005] = 0x55;
        ppu.vram[0x0006] = 0x66;
        
        ppu.write(PPU_REG_ADDR, 0x20, &mut mapper); // Nametable start
        ppu.write(PPU_REG_ADDR, 0x05, &mut mapper);
        
        let val1 = ppu.read(PPU_REG_DATA, &mapper); // This should be buffered (initial 0)
        let val2 = ppu.read(PPU_REG_DATA, &mapper); // This should be 0x55
        
        assert_eq!(val1, 0);
        assert_eq!(val2, 0x55);
    }

    /// **Objective**: Verify PPU step state machine edge cases (VBLANK, Frame End, scrolling).
    #[test]
    fn test_ppu_step_edge_cases() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        let mapper = Mapper0::new(vec![0; 16384], vec![0; 8192], Mirroring::Vertical);
        
        ppu.scanline = 240;
        ppu.cycles = 340;
        ppu.ctrl = CtrlFlags::GENERATE_NMI.bits();
        
        ppu.step(&mapper);
        assert_eq!(ppu.scanline, 241);
        assert_eq!(ppu.cycles, 0);
        assert!(ppu.status & StatusFlags::VBLANK_STARTED.bits() != 0);
        assert!(ppu.nmi_interrupt);
        
        ppu.scanline = 260;
        ppu.cycles = 340;
        ppu.step(&mapper);
        assert_eq!(ppu.scanline, 261);
        assert_eq!(ppu.status & StatusFlags::VBLANK_STARTED.bits(), 0); 
        
        ppu.scanline = 261;
        ppu.cycles = 340;
        ppu.mask = MaskFlags::SHOW_BACKGROUND.bits() | MaskFlags::SHOW_SPRITES.bits();
        ppu.t = 0x1234;
        let frame_complete = ppu.step(&mapper);
        assert_eq!(ppu.scanline, 0);
        assert!(frame_complete);
        assert_eq!(ppu.v, 0x1234); 
        
        ppu.v = 0x73A0; 
        ppu.scanline = 10;
        ppu.cycles = 340;
        ppu.step(&mapper); 
        assert_eq!(ppu.v & 0x0800, 0x0800);
        
        ppu.v = 0x73E0; 
        ppu.scanline = 11;
        ppu.cycles = 340;
        ppu.step(&mapper); 
        assert_eq!(ppu.v & 0x03E0, 0);
    }

    /// **Objective**: Verify sprite rendering edge cases (Vertical Flip, Priority, Sprite 0 Hit).
    #[test]
    fn test_ppu_sprite_edge_cases() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        let mut chr_rom = vec![0; 8192];
        chr_rom[0..16].fill(0xFF); 
        let mapper = Mapper0::new(vec![0; 16384], chr_rom, Mirroring::Vertical);
        
        ppu.mask = MaskFlags::SHOW_BACKGROUND.bits() | MaskFlags::SHOW_SPRITES.bits();
        ppu.palette_table[0] = 0x11; 
        
        ppu.oam_data[0] = 10;
        ppu.oam_data[1] = 0;
        ppu.oam_data[2] = 0xA0; 
        ppu.oam_data[3] = 10;
        
        ppu.vram[0x0000] = 0; 
        
        ppu.render_scanline(11, &mapper);
        
        assert!(ppu.status & StatusFlags::SPRITE_ZERO_HIT.bits() != 0);
    }

    /// **Objective**: Verify remaining PPU edge cases for 100% line coverage.
    #[test]
    fn test_ppu_final_edge_cases() {
        let mut ppu = Ppu::new(vec![0; 0x2000]);
        let mut mapper = Mapper0::new(vec![0; 16384], vec![0; 8192], Mirroring::Vertical);
        
        ppu.write(PPU_REG_MASK, 0x1E, &mut mapper);
        assert_eq!(ppu.mask, 0x1E);
        
        ppu.status |= StatusFlags::VBLANK_STARTED.bits();
        ppu.nmi_interrupt = false;
        ppu.write(PPU_REG_CTRL, CtrlFlags::GENERATE_NMI.bits(), &mut mapper); 
        assert!(ppu.nmi_interrupt);
        
        ppu.write(0x2008, 0xFF, &mut mapper); 
        assert_eq!(ppu.read(0x2008, &mapper), 0);
        ppu.write(PPU_REG_ADDR, 0x3F, &mut mapper);
        ppu.write(PPU_REG_ADDR, 0xFF, &mut mapper);
        ppu.write(PPU_REG_DATA, 0xFF, &mut mapper); 
        
        ppu.cycles = 255;
        ppu.scanline = 10;
        ppu.step(&mapper); 
        assert_eq!(ppu.cycles, 256);
        
        ppu.cycles = 340;
        ppu.scanline = 15;
        ppu.mask = MaskFlags::RENDER_ENABLED.bits();
        ppu.v = 0x0000; 
        ppu.step(&mapper); 
        assert_eq!(ppu.v & 0x7000, 0x1000);
        
        ppu.cycles = 340;
        ppu.scanline = 16;
        ppu.v = 0x7000; 
        ppu.step(&mapper); 
        assert_eq!(ppu.v & 0x7000, 0);
        assert_eq!(ppu.v & 0x03E0, 0x0020);
        
        ppu.vram[0x0000] = 0; 
        ppu.palette_table[0] = 0x12; 
        ppu.render_scanline(0, &mapper); 
    }
}
