use std::fs;

fn main() {
    let data = fs::read("src/assets/Super Mario Bros. (World).nes").unwrap();
    // Pattern: CE 87 07 10 (any) A9 15 8D 87 07
    // or CE 87 07 10 (any) A9 18 8D 87 07 (if 24 frames)
    
    for i in 0..data.len() - 10 {
        if data[i] == 0xCE && data[i+1] == 0x87 && data[i+2] == 0x07 {
            println!("Found DEC $0787 at offset {:#X}", i);
            if data[i+3] == 0x10 {
                println!("Found BPL at offset {:#X}", i+3);
                // Search forward for LDA #$15 or LDA #$18
                for j in i+4..i+10 {
                    if data[j] == 0xA9 {
                        println!("Found LDA # at offset {:#X}, value: {:#X}", j, data[j+1]);
                    }
                }
            }
        }
    }
}
