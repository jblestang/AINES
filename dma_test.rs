struct Bus {
    ppu_oam: [u8; 256],
    ram: [u8; 2048]
}
impl Bus {
    fn read(&mut self, addr: u16) -> u8 { self.ram[(addr & 2047) as usize] }
    fn write(&mut self, addr: u16, data: u8) {
        match addr {
            0x4014 => {
                let mut buffer = [0; 256];
                let hi = (data as u16) << 8;
                for i in 0..256u16 {
                    buffer[i as usize] = self.read(hi + i);
                }
                self.ppu_oam = buffer;
            }
            _ => {}
        }
    }
}
fn main() {}
