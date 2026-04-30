/// # AINES × nestest Integration Test
///
/// Runs the industry-standard nestest.nes ROM in "automation mode" (PC forced to $C000)
/// and compares every CPU state snapshot against the reference nestest.log produced by
/// Nintendulator — the most cycle-accurate NES reference emulator.
///
/// A pass here proves correctness of:
///  - All official 6502 opcodes
///  - Addressing modes (Immediate, ZeroPage, ZeroPageX/Y, Absolute, AbsoluteX/Y, IndirectX/Y)
///  - Flag logic (N, V, B, D, I, Z, C)
///  - Stack operations
///  - Cycle-accurate timing counts
///
/// Reference: https://www.qmtpro.com/~nes/misc/nestest.txt

// Pull in the core modules via the library path trick.
// We re-expose core as a module inside the integration test via a path directive.
use std::fs;

// We need direct access to the core modules. Since this is an integration test
// and the crate is a binary (not a lib), we replicate what main.rs does.
// The integration test binary shares the same source tree.
#[path = "../src/core/mod.rs"]
mod core;

use core::bus::Bus;
use core::cartridge::Cartridge;
use core::cpu::Cpu;

/// Parse a single nestest.log line into its expected CPU state fields.
/// Format: `C000  4C F5 C5  JMP $C5F5  ...  A:00 X:00 Y:00 P:24 SP:FD PPU:  0, 21 CYC:7`
struct LogLine {
    pc: u16,
    a: u8,
    x: u8,
    y: u8,
    p: u8,
    sp: u8,
    cyc: u64,
}

fn parse_log_line(line: &str) -> Option<LogLine> {
    // PC is always the first 4 hex chars
    let pc = u16::from_str_radix(line.get(0..4)?, 16).ok()?;

    // Registers are in the right-hand side after column 48
    let right = line.get(48..)?;

    let a  = parse_field(right, "A:")?;
    let x  = parse_field(right, "X:")?;
    let y  = parse_field(right, "Y:")?;
    let p  = parse_field(right, "P:")?;
    let sp = parse_field(right, "SP:")?;
    let cyc = parse_cyc(right)?;

    Some(LogLine { pc, a, x, y, p, sp, cyc })
}

fn parse_field(s: &str, prefix: &str) -> Option<u8> {
    let pos = s.find(prefix)? + prefix.len();
    u8::from_str_radix(s.get(pos..pos + 2)?, 16).ok()
}

fn parse_cyc(s: &str) -> Option<u64> {
    let pos = s.find("CYC:")? + 4;
    s[pos..].trim().parse().ok()
}

#[test]
fn test_nestest_official_opcodes() {
    // ── Load ROM ──────────────────────────────────────────────────────────────
    let rom_path = "src/assets/nestest.nes";
    let log_path = "src/assets/nestest.log";

    let rom_bytes = fs::read(rom_path)
        .unwrap_or_else(|e| panic!("Cannot read nestest ROM at '{rom_path}': {e}"));
    let log_text = fs::read_to_string(log_path)
        .unwrap_or_else(|e| panic!("Cannot read nestest log at '{log_path}': {e}"));

    let cartridge = Cartridge::load_rom(&rom_bytes)
        .expect("nestest.nes failed to parse");

    let mut bus = Bus::new(cartridge);
    let mut cpu = Cpu::new();

    // ── Automation mode: force PC to $C000 ───────────────────────────────────
    // nestest skips the reset vector in automation mode and starts at $C000.
    // The reset initialises registers; we then override PC.
    cpu.reset(&mut bus);
    cpu.pc = 0xC000;

    // ── Run and compare ───────────────────────────────────────────────────────
    let mut total_cycles: u64 = 7; // nestest log starts at CYC:7 (reset takes 7 cycles)
    let mut failures: Vec<String> = Vec::new();
    const MAX_FAILURES: usize = 5; // Stop early after this many mismatches

    for (line_no, raw_line) in log_text.lines().enumerate() {
        if raw_line.trim().is_empty() { continue; }

        let expected = match parse_log_line(raw_line) {
            Some(l) => l,
            None => {
                eprintln!("Line {}: could not parse: {}", line_no + 1, raw_line);
                continue;
            }
        };

        // ── Snapshot CPU state BEFORE stepping ───────────────────────────────
        let got_pc  = cpu.pc;
        let got_a   = cpu.a;
        let got_x   = cpu.x;
        let got_y   = cpu.y;
        let got_p   = cpu.status.bits();
        let got_sp  = cpu.sp;
        let got_cyc = total_cycles;

        // Compare
        let mut mismatch = false;
        let mut report   = String::new();

        macro_rules! chk {
            ($field:expr, $got:expr, $exp:expr) => {
                if $got != $exp {
                    mismatch = true;
                    report.push_str(&format!(
                        "  {}: got={:#04X} exp={:#04X}\n", $field, $got, $exp
                    ));
                }
            };
        }

        chk!("PC",  got_pc,  expected.pc);
        chk!("A",   got_a,   expected.a);
        chk!("X",   got_x,   expected.x);
        chk!("Y",   got_y,   expected.y);
        chk!("P",   got_p,   expected.p);
        chk!("SP",  got_sp,  expected.sp);
        chk!("CYC", got_cyc, expected.cyc);

        if mismatch {
            failures.push(format!(
                "Line {:4} | {}\n{}",
                line_no + 1,
                raw_line.trim(),
                report
            ));
            if failures.len() >= MAX_FAILURES { break; }
        }

        // ── Step the CPU forward ──────────────────────────────────────────────
        let cycles = cpu.step(&mut bus);
        total_cycles += u64::from(cycles);
    }

    // ── Report ────────────────────────────────────────────────────────────────
    // Check nestest error codes written to $02/$03 in zero-page
    let err_code_official  = bus.read(0x0002);
    let err_code_unofficial = bus.read(0x0003);

    println!("\n══════════════════════════════════════════");
    println!(" AINES × nestest Results");
    println!("══════════════════════════════════════════");
    println!(" Official opcode error code  ($02): {:#04X}", err_code_official);
    println!(" Unofficial opcode err code  ($03): {:#04X}", err_code_unofficial);
    println!(" Log lines checked: {}", log_text.lines().count());
    println!(" State mismatches : {}", failures.len());

    if !failures.is_empty() {
        for f in &failures {
            eprintln!("\n{f}");
        }
    }

    assert!(
        failures.is_empty(),
        "\n{} CPU state mismatch(es) found against nestest.log.\n\
         First failure:\n{}",
        failures.len(),
        failures.first().unwrap()
    );

    assert_eq!(
        err_code_official, 0,
        "nestest official opcode test FAILED — error code ${:02X} written to $02",
        err_code_official
    );
}
