use std::fs;

fn main() {
    let data = fs::read("src/assets/Super Mario Bros. (World).nes").unwrap();
    // Pattern: C6 87 10 (any) A9 15 85 87
    
    for i in 0..data.len() - 8 {
        if data[i] == 0xC6 && data[i+1] == 0x87 {
            println!("Found DEC $87 at offset {:#X}", i);
            if data[i+2] == 0x10 {
                println!("Found BPL at offset {:#X}", i+2);
                for j in i+3..i+8 {
                    if data[j] == 0xA9 {
                        println!("Found LDA # at offset {:#X}, value: {:#X}", j, data[j+1]);
                    }
                }
            }
        }
    }
}
