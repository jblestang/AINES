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

pub struct Ppu {
    pub chr_rom: Vec<u8>,
    pub palette_table: [u8; 32],
    pub vram: [u8; 2048],
    pub oam_data: [u8; 256],
    
    // Registers
    pub ctrl: u8,
    pub mask: u8,
    pub status: u8,
    
    // Scrolling registers (Loopy)
    pub v: u16, // Current VRAM address (15 bits)
    pub t: u16, // Temporary VRAM address (15 bits)
    pub x: u8,   // Fine X scroll (3 bits)
    pub w: bool, // Write latch
    
    pub oam_addr: u8,
    pub nmi_interrupt: bool,

    internal_data_buf: u8,
    
    pub frame_buffer: [u8; 256 * 240 * 4], // RGBA
    pub vertical_mirroring: bool,
    scanline: u16,
    cycles: usize,
}

impl Ppu {
    pub fn mirror_vram_addr(&self, addr: u16) -> u16 {
        let addr = (addr - 0x2000) % 0x1000;
        if self.vertical_mirroring {
            // Vertical: NT0 mirror NT2, NT1 mirror NT3
            addr % 0x0800
        } else {
            // Horizontal: NT0 mirror NT1, NT2 mirror NT3
            if addr < 0x0800 {
                addr % 0x0400
            } else {
                (addr % 0x0400) + 0x0400
            }
        }
    }

    pub fn new(chr_rom: Vec<u8>) -> Self {
        Ppu {
            chr_rom,
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
            
            frame_buffer: [0; 256 * 240 * 4],
            vertical_mirroring: true, // Default
            scanline: 0,
            cycles: 0,
        }
    }

    pub fn write_to_ctrl(&mut self, value: u8) {
        let nmi_before = self.ctrl & 0b1000_0000 != 0;
        self.ctrl = value;
        let nmi_after = self.ctrl & 0b1000_0000 != 0;
        if !nmi_before && nmi_after && (self.status & 0b1000_0000 != 0) {
            self.nmi_interrupt = true;
        }
        // Update temporary VRAM address with nametable bits
        self.t = (self.t & 0xF3FF) | ((value as u16 & 0x03) << 10);
    }

    pub fn write_to_scroll(&mut self, value: u8) {
        if !self.w {
            self.t = (self.t & 0xFFE0) | (value as u16 >> 3);
            self.x = value & 0x07;
            self.w = true;
        } else {
            self.t = (self.t & 0x8C1F) | ((value as u16 & 0x07) << 12) | ((value as u16 & 0xF8) << 2);
            self.w = false;
            // println!("SCROLL Y set to {}, t: {:04X}", value, self.t);
        }
    }

    pub fn write_to_addr(&mut self, value: u8) {
        if !self.w {
            self.t = (self.t & 0x00FF) | ((value as u16 & 0x3F) << 8);
            self.w = true;
        } else {
            self.t = (self.t & 0xFF00) | (value as u16);
            self.v = self.t;
            self.w = false;
            // println!("PPU ADDR set to {:04X}", self.v);
        }
    }

    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x2002 => {
                let status = self.status;
                self.status &= 0x7F; // Clear VBlank ONLY (bit 7)
                self.w = false; // Reset address latch
                status
            }
            0x2004 => self.oam_data[self.oam_addr as usize],
            0x2007 => {
                let mut data = self.internal_data_buf;
                let addr = self.v & 0x3FFF;
                
                // Read from VRAM into the buffer
                self.internal_data_buf = match addr {
                    0x0000..=0x1FFF => self.chr_rom[addr as usize],
                    0x2000..=0x2FFF => self.vram[self.mirror_vram_addr(addr) as usize],
                    _ => 0,
                };
                
                // Palette read is immediate, no buffer delay
                if addr >= 0x3F00 && addr <= 0x3FFF {
                    let mut pal_addr = addr & 0x001F;
                    if pal_addr == 0x10 || pal_addr == 0x14 || pal_addr == 0x18 || pal_addr == 0x1C {
                        pal_addr -= 0x10;
                    }
                    data = self.palette_table[pal_addr as usize];
                }
                
                let increment = if self.ctrl & 0b0000_0100 == 0 { 1 } else { 32 };
                self.v = self.v.wrapping_add(increment);
                
                data
            }
            _ => 0,
        }
    }

    pub fn write(&mut self, addr: u16, data: u8) {
        match addr {
            0x2000 => self.write_to_ctrl(data),
            0x2001 => {
                self.mask = data;
                if data & 0b0001_1000 != 0 {
                    static mut DUMPED: bool = false;
                    unsafe {
                        if !DUMPED {
                            print!("PALETTES: ");
                            for i in 0..32 {
                                print!("{:02X} ", self.palette_table[i]);
                            }
                            println!();
                            DUMPED = true;
                        }
                    }
                }
            }
            0x2003 => self.oam_addr = data,
            0x2004 => {
                self.oam_data[self.oam_addr as usize] = data;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            0x2005 => self.write_to_scroll(data),
            0x2006 => self.write_to_addr(data),
            0x2007 => {
                let addr = self.v & 0x3FFF;
                match addr {
                    0x0000..=0x1FFF => { /* CHR ROM is generally read-only */ }
                    0x2000..=0x2FFF => self.vram[self.mirror_vram_addr(addr) as usize] = data,
                    0x3F00..=0x3FFF => {
                        let mut pal_addr = addr & 0x001F;
                        // Addresses $3F10/$3F14/$3F18/$3F1C are mirrors of $3F00/$3F04/$3F08/$3F0C
                        if pal_addr == 0x10 || pal_addr == 0x14 || pal_addr == 0x18 || pal_addr == 0x1C {
                            pal_addr -= 0x10;
                        }
                        self.palette_table[pal_addr as usize] = data;
                    }
                    _ => {}
                }
                
                let increment = if self.ctrl & 0b0000_0100 == 0 { 1 } else { 32 };
                self.v = self.v.wrapping_add(increment);
            }
            _ => {}
        }
    }

    pub fn step(&mut self) -> bool {
        self.cycles += 1;

        if self.cycles == 256 {
            if self.scanline < 240 {
                self.render_scanline(self.scanline);
            }
        }

        if self.cycles >= 341 {
            self.cycles = 0;
            
            if self.scanline < 240 {
                if self.mask & 0b0001_1000 != 0 {
                    // Increment vertical scroll at the end of each visible scanline
                    if (self.v & 0x7000) != 0x7000 {
                        self.v += 0x1000;
                    } else {
                        self.v &= !0x7000;
                        let mut y_coarse = (self.v & 0x03E0) >> 5;
                        if y_coarse == 29 {
                            y_coarse = 0;
                            self.v ^= 0x0800; // Switch vertical nametable
                        } else if y_coarse == 31 {
                            y_coarse = 0;
                        } else {
                            y_coarse += 1;
                        }
                        self.v = (self.v & !0x03E0) | (y_coarse << 5);
                    }
                    // Reset horizontal scroll bits from t
                    self.v = (self.v & 0xFBE0) | (self.t & 0x041F);
                }
            }

            self.scanline += 1;

            if self.scanline == 241 && self.cycles == 0 {
                // Set VBlank at start of scanline 241
                self.status |= 0x80;
                if self.ctrl & 0x80 != 0 {
                    self.nmi_interrupt = true;
                }
            }

            if self.scanline == 261 {
                self.status &= 0b0011_1111; // Clear VBlank and Sprite 0 Hit
            }

            if self.scanline >= 262 {
                self.scanline = 0;
                // RELOAD V FROM T HERE (Start of new frame)
                if self.mask & 0b0001_1000 != 0 {
                    self.v = self.t;
                }
                return true; // Frame is complete
            }
        }
        false
    }

    fn render_scanline(&mut self, y: u16) {
        if y == 0 {
             // Basic frame info once per frame
             // let nt1_addr = self.mirror_vram_addr(0x2400) as usize;
             let s0_y = self.oam_data[0];
             let s1_y = self.oam_data[4];
             if s0_y < 240 || s1_y < 240 {
                // println!("FRAME START - Sprite0 Y: {}, Sprite1 Y: {}, NT1[0]: {:02X}", s0_y, s1_y, self.vram[nt1_addr]);
             }
        }

        if y % 60 == 0 {
             // println!("SCANLINE {}: V={:04X}, NT={}, CoarseY={}, FineY={}", y, self.v, nt, coarse_y, fine_y);
        }

        if y >= 240 { return; }

        let bg_bank = if self.ctrl & 0b0001_0000 != 0 { 0x1000 } else { 0x0000 };
        let mut bg_opaque = [false; 256];
        
        // 2. BACKGROUND RENDERING
        if self.mask & 0b0000_1000 != 0 {
            let fine_x = self.x as u16;
            for screen_x in 0..256u16 {
                let total_x = screen_x + fine_x;
                let coarse_x_inc = total_x / 8;
                let base_coarse_x = self.v & 0x1F;
                let final_coarse_x = (base_coarse_x + coarse_x_inc) % 32;
                let final_nt = ((self.v >> 10) & 0x03) ^ ((base_coarse_x + coarse_x_inc) / 32) as u16;
                let v = (self.v & !0x041F) | (final_nt << 10) | final_coarse_x;

                let coarse_x = v & 0x1F;
                let coarse_y = (v >> 5) & 0x1F;
                let nt_select = (v >> 10) & 0x03;
                let fine_y = (v >> 12) & 0x07;
                
                let nt_addr = 0x2000 | (nt_select << 10) | (coarse_y << 5) | coarse_x;
                let vram_idx = self.mirror_vram_addr(nt_addr) as usize;
                let tile_id = if vram_idx < self.vram.len() { self.vram[vram_idx] as u16 } else { 0 };
                
                let attr_addr = 0x23C0 | (nt_select << 10) | ((coarse_y >> 2) << 3) | (coarse_x >> 2);
                let attr_idx = self.mirror_vram_addr(attr_addr) as usize;
                let attr_byte = if attr_idx < self.vram.len() { self.vram[attr_idx] } else { 0 };
                let shift = ((coarse_y & 2) << 1) | (coarse_x & 2);
                let palette_idx = (attr_byte >> shift) & 0x03;
                
                let tile_addr = bg_bank + tile_id * 16 + fine_y;
                let mut p_low = 0;
                let mut p_high = 0;
                if ((tile_addr + 8) as usize) < self.chr_rom.len() {
                    p_low = self.chr_rom[tile_addr as usize];
                    p_high = self.chr_rom[(tile_addr + 8) as usize];
                }
                
                let bit_idx = 7 - (total_x % 8);
                let color_val = (((p_high >> bit_idx) & 1) << 1) | ((p_low >> bit_idx) & 1);
                bg_opaque[screen_x as usize] = color_val != 0;
                
                let sys_color_idx = if color_val == 0 { self.palette_table[0] } else { self.palette_table[(palette_idx * 4 + color_val) as usize] };
                let color = SYSTEM_PALETTE[(sys_color_idx & 0x3F) as usize];
                let fb_idx = (y as usize * 256 + screen_x as usize) * 4;
                if fb_idx + 3 < self.frame_buffer.len() {
                    self.frame_buffer[fb_idx] = color.0;
                    self.frame_buffer[fb_idx + 1] = color.1;
                    self.frame_buffer[fb_idx + 2] = color.2;
                    self.frame_buffer[fb_idx + 3] = 255;
                }
            }
        }

        // 3. SPRITE RENDERING
        if self.mask & 0b0001_0000 != 0 {
            let s_bank = if self.ctrl & 0b0000_1000 != 0 { 0x1000 } else { 0x0000 };
            for i in (0..64).rev() {
                let oam_idx = i * 4;
                let sprite_y = self.oam_data[oam_idx] as u16;
                if y >= sprite_y + 1 && y < sprite_y + 9 {
                    let tile_id = self.oam_data[oam_idx + 1] as u16;
                    let attr = self.oam_data[oam_idx + 2];
                    let sprite_x = self.oam_data[oam_idx + 3] as u16;
                    let flip_h = attr & 0b0100_0000 != 0;
                    let flip_v = attr & 0b1000_0000 != 0;
                    let palette_idx = (attr & 0b11) + 4;
                    let row = if flip_v { 7 - (y - (sprite_y + 1)) } else { y - (sprite_y + 1) };
                    let tile_addr = s_bank + tile_id * 16 + row;
                    
                    let mut p_low = 0;
                    let mut p_high = 0;
                    if ((tile_addr + 8) as usize) < self.chr_rom.len() {
                        p_low = self.chr_rom[tile_addr as usize];
                        p_high = self.chr_rom[(tile_addr + 8) as usize];
                    }

                    for dx in 0..8 {
                        let screen_x = sprite_x + dx;
                        if screen_x >= 256 { continue; }
                        let bit_idx = if flip_h { dx } else { 7 - dx };
                        let color_val = (((p_high >> bit_idx) & 1) << 1) | ((p_low >> bit_idx) & 1);
                        
                        if color_val != 0 {
                            // Sprite 0 Hit detection
                            if i == 0 && bg_opaque[screen_x as usize] && (self.mask & 0b0001_1000 == 0b0001_1000) {
                                 if self.status & 0x40 == 0 {
                                     //println!("SPRITE 0 HIT at scanline {}, dot {}", y, screen_x);
                                 }
                                 self.status |= 0x40;
                            }

                            let priority = (attr >> 5) & 1;
                            if priority == 0 || !bg_opaque[screen_x as usize] {
                                let sys_idx = self.palette_table[(palette_idx * 4 + color_val) as usize];
                                let color = SYSTEM_PALETTE[(sys_idx & 0x3F) as usize];
                                let fb_idx = (y as usize * 256 + screen_x as usize) * 4;
                                
                                // Trace Mario (usually Sprite 1 or 2 in SMB)
                                if i == 1 && y % 16 == 0 {
                                    // println!("MARIO RENDER: scanline {}, x {}, fb_idx {}", y, screen_x, fb_idx);
                                }

                                if fb_idx + 3 < self.frame_buffer.len() {
                                    self.frame_buffer[fb_idx] = color.0;
                                    self.frame_buffer[fb_idx + 1] = color.1;
                                    self.frame_buffer[fb_idx + 2] = color.2;
                                    self.frame_buffer[fb_idx + 3] = 255;
                                }
                            }
                        }
                    }
                }
            }
        }

    }
}
