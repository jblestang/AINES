use std::fs;
fn main() {
    let data = fs::read("src/assets/Super Mario Bros. (World).nes").unwrap();
    let prg_rom = &data[16..16+32768];
    let nmi_lo = prg_rom[0x7FFA];
    let nmi_hi = prg_rom[0x7FFB];
    let rst_lo = prg_rom[0x7FFC];
    let rst_hi = prg_rom[0x7FFD];
    println!("NMI: {:02X}{:02X}, RST: {:02X}{:02X}", nmi_hi, nmi_lo, rst_hi, rst_lo);
}
