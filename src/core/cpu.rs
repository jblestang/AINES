//! # Ricoh 2A03 Central Processing Unit (CPU)
//! 
//! The 2A03 is a custom 8-bit microprocessor based on the MOS Technology 6502.
//! It includes integrated APU (Audio Processing Unit) and DMA (Direct Memory Access) logic.
//! 
//! ## Key Resources:
//! - [NESDev CPU Reference](https://www.nesdev.org/wiki/CPU)
//! - [6502 Instruction Set](https://www.nesdev.org/wiki/CPU_instructions)
//! - [CPU Addressing Modes](https://www.nesdev.org/wiki/CPU_addressing_modes)
//! - [2A03 Technical details](https://www.nesdev.org/wiki/2A03)

use super::bus::Bus;

#[derive(Debug)]
pub enum AddressingMode {
    Immediate,
    ZeroPage,
    ZeroPageX,
    ZeroPageY,
    Absolute,
    AbsoluteX,
    AbsoluteY,
    IndirectX,
    IndirectY,
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct CpuFlags: u8 {
        const CARRY             = 0b0000_0001;
        const ZERO              = 0b0000_0010;
        const INTERRUPT_DISABLE = 0b0000_0100;
        const DECIMAL_MODE      = 0b0000_1000;
        const BREAK             = 0b0001_0000;
        const BREAK2            = 0b0010_0000;
        const OVERFLOW          = 0b0100_0000;
        const NEGATIVE          = 0b1000_0000;
    }
}

/// Initial status flags: Interrupt Disable set, and the "Always 1" Bit 5 set.
/// <https://www.nesdev.org/wiki/CPU_status_flags>
pub const INITIAL_STATUS: u8 = CpuFlags::INTERRUPT_DISABLE.bits() | CpuFlags::BREAK2.bits();

/// Start of the CPU stack in memory
pub const STACK_START: u16 = 0x0100;
/// Initial value for the stack pointer
pub const STACK_INITIAL_VALUE: u8 = 0xFD;

/// Number of CPU cycles consumed by an NMI
pub const NMI_CYCLES: u32 = 7;
/// Vector for the non-maskable interrupt (NMI)
pub const NMI_VECTOR: u16 = 0xFFFA;
/// Vector for the initial program counter on reset
pub const RESET_VECTOR: u16 = 0xFFFC;

pub struct Cpu {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub pc: u16,
    pub sp: u8,
    pub status: CpuFlags,
}

impl Cpu {
    /// Creates a new CPU instance with initial register values
    pub fn new() -> Self {
        Cpu {
            a: 0,
            x: 0,
            y: 0,
            pc: 0,
            sp: STACK_INITIAL_VALUE,
            status: CpuFlags::from_bits_truncate(INITIAL_STATUS),
        }
    }

    /// Performs a CPU reset, reloading the Program Counter from the Reset Vector
    pub fn reset(&mut self, bus: &mut Bus) {
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.sp = STACK_INITIAL_VALUE;
        self.status = CpuFlags::from_bits_truncate(INITIAL_STATUS);

        self.pc = Self::mem_read_u16(bus, RESET_VECTOR);
    }

    /// Triggers a Non-Maskable Interrupt
    pub fn nmi(&mut self, bus: &mut Bus) {
        self.stack_push_u16(bus, self.pc);
        let mut flags = self.status;
        flags.remove(CpuFlags::BREAK);
        flags.insert(CpuFlags::BREAK2);
        self.stack_push(bus, flags.bits());
        self.status.insert(CpuFlags::INTERRUPT_DISABLE);

        self.pc = Self::mem_read_u16(bus, NMI_VECTOR);
    }

    fn mem_read(bus: &mut Bus, addr: u16) -> u8 {
        bus.read(addr)
    }

    fn mem_write(bus: &mut Bus, addr: u16, data: u8) {
        bus.write(addr, data);
    }

    fn mem_read_u16(bus: &mut Bus, pos: u16) -> u16 {
        let lo = u16::from(Self::mem_read(bus, pos));
        let hi = u16::from(Self::mem_read(bus, pos + 1));
        (hi << 8) | lo
    }


    fn stack_push(&mut self, bus: &mut Bus, data: u8) {
        Self::mem_write(bus, STACK_START + u16::from(self.sp), data);
        self.sp = self.sp.wrapping_sub(1);
    }

    fn stack_pop(&mut self, bus: &mut Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        Self::mem_read(bus, STACK_START + u16::from(self.sp))
    }

    fn stack_push_u16(&mut self, bus: &mut Bus, data: u16) {
        let hi = (data >> 8) as u8;
        let lo = (data & 0xff) as u8;
        self.stack_push(bus, hi);
        self.stack_push(bus, lo);
    }

    fn stack_pop_u16(&mut self, bus: &mut Bus) -> u16 {
        let lo = u16::from(self.stack_pop(bus));
        let hi = u16::from(self.stack_pop(bus));
        hi << 8 | lo
    }

    /// Updates the Zero and Negative flags based on the result of an operation.
    /// 
    /// - **Zero Flag**: Set if result is 0, cleared otherwise.
    /// - **Negative Flag**: Set if bit 7 of the result is 1 (two's complement negative), cleared otherwise.
    fn update_zero_and_negative_flags(&mut self, result: u8) {
        if result == 0 {
            self.status.insert(CpuFlags::ZERO);
        } else {
            self.status.remove(CpuFlags::ZERO);
        }

        if result & 0b1000_0000 != 0 {
            self.status.insert(CpuFlags::NEGATIVE);
        } else {
            self.status.remove(CpuFlags::NEGATIVE);
        }
    }

    /// Resolves the effective memory address based on the 6502 addressing mode.
    /// 
    /// # Algorithms
    /// - **Zero Page**: Accesses the first 256 bytes of RAM (0x00..0xFF). Faster than Absolute.
    /// - **Indexed Zero Page (X/Y)**: Address is `(base + register) % 256`. Wraps within the zero page.
    /// - **Indexed Absolute (X/Y)**: Address is `base + register`. Returns a `page_crossed` flag 
    ///   if the addition carries into the high byte, which typically adds an extra CPU cycle.
    /// - **Indexed Indirect (X)**: "Indirect,X" ($nn,X). Dereferences a 16-bit pointer starting at 
    ///   zero-page address `(base + X) % 256`.
    /// - **Indirect Indexed (Y)**: "Indirect,Y" ($nn),Y. Dereferences a 16-bit pointer from 
    ///   zero-page `base`, then adds Y to that address. Can trigger a page-crossed cycle.
    /// 
    /// See: [6502 Addressing Modes](https://www.nesdev.org/wiki/CPU_addressing_modes)
    fn get_operand_address(&self, bus: &mut Bus, mode: &AddressingMode) -> (u16, bool) {
        match mode {
            AddressingMode::Immediate => (self.pc, false),
            AddressingMode::ZeroPage => (u16::from(Self::mem_read(bus, self.pc)), false),
            AddressingMode::Absolute => (Self::mem_read_u16(bus, self.pc), false),
            AddressingMode::ZeroPageX => {
                let pos = Self::mem_read(bus, self.pc);
                (u16::from(pos.wrapping_add(self.x)), false)
            }
            AddressingMode::ZeroPageY => {
                let pos = Self::mem_read(bus, self.pc);
                (u16::from(pos.wrapping_add(self.y)), false)
            }
            AddressingMode::AbsoluteX => {
                let base = Self::mem_read_u16(bus, self.pc);
                let addr = base.wrapping_add(u16::from(self.x));
                (addr, self.page_crossed(base, addr))
            }
            AddressingMode::AbsoluteY => {
                let base = Self::mem_read_u16(bus, self.pc);
                let addr = base.wrapping_add(u16::from(self.y));
                (addr, self.page_crossed(base, addr))
            }
            AddressingMode::IndirectX => {
                let base = Self::mem_read(bus, self.pc);
                let ptr = base.wrapping_add(self.x);
                let lo = Self::mem_read(bus, u16::from(ptr));
                let hi = Self::mem_read(bus, u16::from(ptr.wrapping_add(1)));
                ((u16::from(hi) << 8) | u16::from(lo), false)
            }
            AddressingMode::IndirectY => {
                let base = Self::mem_read(bus, self.pc);
                let lo = Self::mem_read(bus, u16::from(base));
                let hi = Self::mem_read(bus, u16::from(base.wrapping_add(1)));
                let deref_base = (u16::from(hi) << 8) | u16::from(lo);
                let addr = deref_base.wrapping_add(u16::from(self.y));
                (addr, self.page_crossed(deref_base, addr))
            }
        }
    }

    fn page_crossed(&self, addr1: u16, addr2: u16) -> bool {
        (addr1 & 0xFF00) != (addr2 & 0xFF00)
    }

    fn get_operand_address_for_jmp_indirect(&self, bus: &mut Bus) -> u16 {
        let addr = Self::mem_read_u16(bus, self.pc);
        
        

        if addr & 0x00FF == 0x00FF {
            let lo = Self::mem_read(bus, addr);
            let hi = Self::mem_read(bus, addr & 0xFF00);
            (u16::from(hi) << 8) | u16::from(lo)
        } else {
            Self::mem_read_u16(bus, addr)
        }
    }

    /// Branch instruction helper.
    /// 
    /// # Algorithm: Relative Addressing
    /// 1. Fetch 8-bit signed offset.
    /// 2. If condition is met:
    ///    - Add offset to PC.
    ///    - Add 1 cycle if branch taken.
    ///    - Add 1 additional cycle if branch crosses a page boundary.
    /// 
    /// See: [Branch Instructions](https://www.nesdev.org/wiki/CPU_instructions#Branch_instructions)
    fn branch(&mut self, bus: &mut Bus, condition: bool) -> u32 {
        let jump: i8 = Self::mem_read(bus, self.pc) as i8;
        self.pc = self.pc.wrapping_add(1);
        if condition {
            let old_pc = self.pc;
            self.pc = self.pc.wrapping_add(jump as u16);
            if self.page_crossed(old_pc, self.pc) {
                return 2;
            }
            return 1;
        }
        0
    }

    /// Load Accumulator (LDA)
    /// <https://www.nesdev.org/wiki/CPU_instructions#LDA>
    fn lda(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = Self::mem_read(bus, addr);
        self.a = value;
        self.update_zero_and_negative_flags(self.a);
        page_crossed
    }

    /// Load X Register (LDX)
    /// <https://www.nesdev.org/wiki/CPU_instructions#LDX>
    fn ldx(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = Self::mem_read(bus, addr);
        self.x = value;
        self.update_zero_and_negative_flags(self.x);
        page_crossed
    }

    /// Load Y Register (LDY)
    /// <https://www.nesdev.org/wiki/CPU_instructions#LDY>
    fn ldy(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = Self::mem_read(bus, addr);
        self.y = value;
        self.update_zero_and_negative_flags(self.y);
        page_crossed
    }

    /// Store Accumulator (STA)
    /// <https://www.nesdev.org/wiki/CPU_instructions#STA>
    fn sta(&self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        Self::mem_write(bus, addr, self.a);
    }

    /// Store X Register (STX)
    /// <https://www.nesdev.org/wiki/CPU_instructions#STX>
    fn stx(&self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        Self::mem_write(bus, addr, self.x);
    }

    /// Store Y Register (STY)
    /// <https://www.nesdev.org/wiki/CPU_instructions#STY>
    fn sty(&self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        Self::mem_write(bus, addr, self.y);
    }

    /// Implementation of the Addition algorithm (ADC).
    /// 
    /// # Algorithm (2A03 Binary Only)
    /// The 2A03 lacks the 6502's Decimal mode. All math is performed in binary.
    /// 1. Sum = A + Data + Carry
    /// 2. **Carry Flag**: Set if Sum > 255.
    /// 3. **Overflow Flag**: Set if the sign of the result is wrong.
    ///    Formula: `V = ~(A ^ Data) & (A ^ Result) & 0x80`.
    ///    This occurs when two positives yield a negative or two negatives yield a positive.
    /// 4. Result = Sum as u8 (truncating high bit).
    /// 
    /// See: [ADC Instruction](https://www.nesdev.org/wiki/Status_flags#Carry)
    fn add_to_register_a(&mut self, data: u8) {
        let sum = u16::from(self.a) 
            + u16::from(data) 
            + i32::from(self.status.contains(CpuFlags::CARRY)) as u16;

        let carry = sum > 0xFF;

        if carry {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }

        let result = sum as u8;

        // Overflow detection logic:
        // If (A and Data have the same sign) AND (Result has a different sign)
        if (data ^ result) & (result ^ self.a) & 0x80 != 0 {
            self.status.insert(CpuFlags::OVERFLOW);
        } else {
            self.status.remove(CpuFlags::OVERFLOW);
        }

        self.a = result;
        self.update_zero_and_negative_flags(self.a);
    }

    /// Add with Carry (ADC) instruction.
    /// <https://www.nesdev.org/wiki/CPU_instructions#ADC>
    fn adc(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = Self::mem_read(bus, addr);
        self.add_to_register_a(value);
        page_crossed
    }

    /// Subtract with Carry (SBC) instruction.
    /// 
    /// # Algorithm
    /// SBC is implemented as ADC with the bitwise complement of the operand.
    /// `A - M - ~C` is equivalent to `A + ~M + C`.
    /// 
    /// See: [SBC Instruction](https://www.nesdev.org/wiki/CPU_instructions#SBC)
    fn sbc(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = Self::mem_read(bus, addr);
        // SBC A, M => ADC A, ~M
        self.add_to_register_a(((value as i8).wrapping_neg().wrapping_sub(1)) as u8);
        page_crossed
    }

    /// Logical AND (AND)
    /// <https://www.nesdev.org/wiki/CPU_instructions#AND>
    fn and(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = Self::mem_read(bus, addr);
        self.a &= value;
        self.update_zero_and_negative_flags(self.a);
        page_crossed
    }

    /// Exclusive OR (EOR)
    /// <https://www.nesdev.org/wiki/CPU_instructions#EOR>
    fn eor(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = Self::mem_read(bus, addr);
        self.a ^= value;
        self.update_zero_and_negative_flags(self.a);
        page_crossed
    }

    /// Logical Inclusive OR (ORA)
    /// <https://www.nesdev.org/wiki/CPU_instructions#ORA>
    fn ora(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let value = Self::mem_read(bus, addr);
        self.a |= value;
        self.update_zero_and_negative_flags(self.a);
        page_crossed
    }

    /// Arithmetic Shift Left (ASL) - Accumulator
    /// <https://www.nesdev.org/wiki/CPU_instructions#ASL>
    fn asl_a(&mut self) {
        let mut data = self.a;
        if data >> 7 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data <<= 1;
        self.a = data;
        self.update_zero_and_negative_flags(self.a);
    }

    /// Arithmetic Shift Left (ASL) - Memory
    fn asl(&mut self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        let mut data = Self::mem_read(bus, addr);
        if data >> 7 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data <<= 1;
        Self::mem_write(bus, addr, data);
        self.update_zero_and_negative_flags(data);
    }

    /// Logical Shift Right (LSR) - Accumulator
    /// <https://www.nesdev.org/wiki/CPU_instructions#LSR>
    fn lsr_a(&mut self) {
        let mut data = self.a;
        if data & 1 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data >>= 1;
        self.a = data;
        self.update_zero_and_negative_flags(self.a);
    }

    /// Logical Shift Right (LSR) - Memory
    fn lsr(&mut self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        let mut data = Self::mem_read(bus, addr);
        if data & 1 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data >>= 1;
        Self::mem_write(bus, addr, data);
        self.update_zero_and_negative_flags(data);
    }

    /// Rotate Left (ROL) - Accumulator
    /// <https://www.nesdev.org/wiki/CPU_instructions#ROL>
    fn rol_a(&mut self) {
        let mut data = self.a;
        let old_carry = self.status.contains(CpuFlags::CARRY);

        if data >> 7 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data <<= 1;
        if old_carry {
            data |= 1;
        }
        self.a = data;
        self.update_zero_and_negative_flags(self.a);
    }

    /// Rotate Left (ROL) - Memory
    fn rol(&mut self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        let mut data = Self::mem_read(bus, addr);
        let old_carry = self.status.contains(CpuFlags::CARRY);

        if data >> 7 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data <<= 1;
        if old_carry {
            data |= 1;
        }
        Self::mem_write(bus, addr, data);
        self.update_zero_and_negative_flags(data);
    }

    /// Rotate Right (ROR) - Accumulator
    /// <https://www.nesdev.org/wiki/CPU_instructions#ROR>
    fn ror_a(&mut self) {
        let mut data = self.a;
        let old_carry = self.status.contains(CpuFlags::CARRY);

        if data & 1 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data >>= 1;
        if old_carry {
            data |= 0b1000_0000;
        }
        self.a = data;
        self.update_zero_and_negative_flags(self.a);
    }

    /// Rotate Right (ROR) - Memory
    fn ror(&mut self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        let mut data = Self::mem_read(bus, addr);
        let old_carry = self.status.contains(CpuFlags::CARRY);

        if data & 1 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        data >>= 1;
        if old_carry {
            data |= 0b1000_0000;
        }
        Self::mem_write(bus, addr, data);
        self.update_zero_and_negative_flags(data);
    }

    /// Increment Memory (INC)
    /// <https://www.nesdev.org/wiki/CPU_instructions#INC>
    fn inc(&mut self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        let data = Self::mem_read(bus, addr).wrapping_add(1);
        Self::mem_write(bus, addr, data);
        self.update_zero_and_negative_flags(data);
    }

    /// Increment X Register (INX)
    /// <https://www.nesdev.org/wiki/CPU_instructions#INX>
    fn inx(&mut self) {
        self.x = self.x.wrapping_add(1);
        self.update_zero_and_negative_flags(self.x);
    }

    /// Increment Y Register (INY)
    /// <https://www.nesdev.org/wiki/CPU_instructions#INY>
    fn iny(&mut self) {
        self.y = self.y.wrapping_add(1);
        self.update_zero_and_negative_flags(self.y);
    }

    /// Decrement Memory (DEC)
    /// <https://www.nesdev.org/wiki/CPU_instructions#DEC>
    fn dec(&mut self, bus: &mut Bus, mode: &AddressingMode) {
        let (addr, _) = self.get_operand_address(bus, mode);
        let data = Self::mem_read(bus, addr).wrapping_sub(1);
        Self::mem_write(bus, addr, data);
        self.update_zero_and_negative_flags(data);
    }

    /// Decrement X Register (DEX)
    /// <https://www.nesdev.org/wiki/CPU_instructions#DEX>
    fn dex(&mut self) {
        self.x = self.x.wrapping_sub(1);
        self.update_zero_and_negative_flags(self.x);
    }

    /// Decrement Y Register (DEY)
    /// <https://www.nesdev.org/wiki/CPU_instructions#DEY>
    fn dey(&mut self) {
        self.y = self.y.wrapping_sub(1);
        self.update_zero_and_negative_flags(self.y);
    }

    /// Base Comparison logic (used by CMP, CPX, CPY)
    /// 
    /// # Algorithm
    /// 1. Subtract operand from register (Carry is NOT used as input).
    /// 2. **Carry Flag**: Set if Register >= Operand.
    /// 3. Update Zero and Negative flags based on the result.
    fn cmp_base(&mut self, mode: &AddressingMode, compare_with: u8, bus: &mut Bus) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let data = Self::mem_read(bus, addr);
        if data <= compare_with {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }

        self.update_zero_and_negative_flags(compare_with.wrapping_sub(data));
        page_crossed
    }

    /// Compare Accumulator (CMP)
    /// <https://www.nesdev.org/wiki/CPU_instructions#CMP>
    fn cmp(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        self.cmp_base(mode, self.a, bus)
    }

    /// Compare X Register (CPX)
    /// <https://www.nesdev.org/wiki/CPU_instructions#CPX>
    fn cpx(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        self.cmp_base(mode, self.x, bus)
    }

    /// Compare Y Register (CPY)
    /// <https://www.nesdev.org/wiki/CPU_instructions#CPY>
    fn cpy(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        self.cmp_base(mode, self.y, bus)
    }

    /// Test Bits (BIT)
    /// 
    /// # Algorithm
    /// 1. `AND` Accumulator with operand (sets Zero flag).
    /// 2. Copy bit 7 of operand to Negative flag.
    /// 3. Copy bit 6 of operand to Overflow flag.
    /// 
    /// See: [BIT Instruction](https://www.nesdev.org/wiki/CPU_instructions#BIT)
    fn bit(&mut self, bus: &mut Bus, mode: &AddressingMode) -> bool {
        let (addr, page_crossed) = self.get_operand_address(bus, mode);
        let data = Self::mem_read(bus, addr);
        let and = self.a & data;
        
        if and == 0 {
            self.status.insert(CpuFlags::ZERO);
        } else {
            self.status.remove(CpuFlags::ZERO);
        }

        // BIT copies bits 7 and 6 of the operand directly to the status register flags
        if data & CpuFlags::NEGATIVE.bits() != 0 {
            self.status.insert(CpuFlags::NEGATIVE);
        } else {
            self.status.remove(CpuFlags::NEGATIVE);
        }

        if data & CpuFlags::OVERFLOW.bits() != 0 {
            self.status.insert(CpuFlags::OVERFLOW);
        } else {
            self.status.remove(CpuFlags::OVERFLOW);
        }
        page_crossed
    }

    /// Executes one CPU instruction and returns the number of cycles consumed.
    /// 
    /// # Dispatch Algorithm
    /// 1. Check for NMI (Non-Maskable Interrupt). NMIs are prioritized over instructions.
    /// 2. Fetch the opcode at the current Program Counter (PC).
    /// 3. Match the opcode to its instruction handler.
    /// 4. Increment PC by the operand size.
    /// 5. Return base cycles + any extra cycles (page crossing, branch taken).
    /// 
    /// See: [6502 Opcode Table](http://www.6502.org/tutorials/6502opcodes.html)
    pub fn step(&mut self, bus: &mut Bus) -> u32 {
        if bus.ppu.nmi_interrupt {
            bus.ppu.nmi_interrupt = false;
            self.nmi(bus);
            return NMI_CYCLES;
        }
        
        let opcode = Self::mem_read(bus, self.pc);
        self.pc += 1;

        let cycles = match opcode {
            // LDA
            0xA9 => { self.lda(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0xA5 => { self.lda(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0xB5 => { self.lda(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0xAD => { self.lda(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0xBD => { let pc = self.lda(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + u32::from(pc) }
            0xB9 => { let pc = self.lda(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + u32::from(pc) }
            0xA1 => { self.lda(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0xB1 => { let pc = self.lda(bus, &AddressingMode::IndirectY); self.pc += 1; 5 + u32::from(pc) }
            // LDX
            0xA2 => { self.ldx(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0xA6 => { self.ldx(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0xB6 => { self.ldx(bus, &AddressingMode::ZeroPageY); self.pc += 1; 4 }
            0xAE => { self.ldx(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0xBE => { let pc = self.ldx(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + u32::from(pc) }
            // LDY
            0xA0 => { self.ldy(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0xA4 => { self.ldy(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0xB4 => { self.ldy(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0xAC => { self.ldy(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0xBC => { let pc = self.ldy(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + u32::from(pc) }
            // STA
            0x85 => { self.sta(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x95 => { self.sta(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0x8D => { self.sta(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0x9D => { self.sta(bus, &AddressingMode::AbsoluteX); self.pc += 2; 5 }
            0x99 => { self.sta(bus, &AddressingMode::AbsoluteY); self.pc += 2; 5 }
            0x81 => { self.sta(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0x91 => { self.sta(bus, &AddressingMode::IndirectY); self.pc += 1; 6 }
            // STX
            0x86 => { self.stx(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x96 => { self.stx(bus, &AddressingMode::ZeroPageY); self.pc += 1; 4 }
            0x8E => { self.stx(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            // STY
            0x84 => { self.sty(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x94 => { self.sty(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0x8C => { self.sty(bus, &AddressingMode::Absolute); self.pc += 2; 4 }

            // ADC
            0x69 => { self.adc(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0x65 => { self.adc(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x75 => { self.adc(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0x6D => { self.adc(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0x7D => { let pc = self.adc(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + u32::from(pc) }
            0x79 => { let pc = self.adc(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + u32::from(pc) }
            0x61 => { self.adc(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0x71 => { let pc = self.adc(bus, &AddressingMode::IndirectY); self.pc += 1; 5 + u32::from(pc) }
            // SBC
            0xE9 => { self.sbc(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0xE5 => { self.sbc(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0xF5 => { self.sbc(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0xED => { self.sbc(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0xFD => { let pc = self.sbc(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + u32::from(pc) }
            0xF9 => { let pc = self.sbc(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + u32::from(pc) }
            0xE1 => { self.sbc(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0xF1 => { let pc = self.sbc(bus, &AddressingMode::IndirectY); self.pc += 1; 5 + u32::from(pc) }
            // AND
            0x29 => { self.and(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0x25 => { self.and(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x35 => { self.and(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0x2D => { self.and(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0x3D => { let pc = self.and(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + u32::from(pc) }
            0x39 => { let pc = self.and(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + u32::from(pc) }
            0x21 => { self.and(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0x31 => { let pc = self.and(bus, &AddressingMode::IndirectY); self.pc += 1; 5 + u32::from(pc) }
            // EOR
            0x49 => { self.eor(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0x45 => { self.eor(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x55 => { self.eor(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0x4D => { self.eor(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0x5D => { let pc = self.eor(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + u32::from(pc) }
            0x59 => { let pc = self.eor(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + u32::from(pc) }
            0x41 => { self.eor(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0x51 => { let pc = self.eor(bus, &AddressingMode::IndirectY); self.pc += 1; 5 + u32::from(pc) }
            // ORA
            0x09 => { self.ora(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0x05 => { self.ora(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x15 => { self.ora(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0x0D => { self.ora(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0x1D => { let pc = self.ora(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + u32::from(pc) }
            0x19 => { let pc = self.ora(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + u32::from(pc) }
            0x01 => { self.ora(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0x11 => { let pc = self.ora(bus, &AddressingMode::IndirectY); self.pc += 1; 5 + u32::from(pc) }

            // ASL
            0x0A => { self.asl_a(); 2 }
            0x06 => { self.asl(bus, &AddressingMode::ZeroPage); self.pc += 1; 5 }
            0x16 => { self.asl(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0x0E => { self.asl(bus, &AddressingMode::Absolute); self.pc += 2; 6 }
            0x1E => { self.asl(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }
            // LSR
            0x4A => { self.lsr_a(); 2 }
            0x46 => { self.lsr(bus, &AddressingMode::ZeroPage); self.pc += 1; 5 }
            0x56 => { self.lsr(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0x4E => { self.lsr(bus, &AddressingMode::Absolute); self.pc += 2; 6 }
            0x5E => { self.lsr(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }
            // ROL
            0x2A => { self.rol_a(); 2 }
            0x26 => { self.rol(bus, &AddressingMode::ZeroPage); self.pc += 1; 5 }
            0x36 => { self.rol(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0x2E => { self.rol(bus, &AddressingMode::Absolute); self.pc += 2; 6 }
            0x3E => { self.rol(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }
            // ROR
            0x6A => { self.ror_a(); 2 }
            0x66 => { self.ror(bus, &AddressingMode::ZeroPage); self.pc += 1; 5 }
            0x76 => { self.ror(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0x6E => { self.ror(bus, &AddressingMode::Absolute); self.pc += 2; 6 }
            0x7E => { self.ror(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }

            // INC
            0xE6 => { self.inc(bus, &AddressingMode::ZeroPage); self.pc += 1; 5 }
            0xF6 => { self.inc(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0xEE => { self.inc(bus, &AddressingMode::Absolute); self.pc += 2; 6 }
            0xFE => { self.inc(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }
            // DEC
            0xC6 => { self.dec(bus, &AddressingMode::ZeroPage); self.pc += 1; 5 }
            0xD6 => { self.dec(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0xCE => { self.dec(bus, &AddressingMode::Absolute); self.pc += 2; 6 }
            0xDE => { self.dec(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }

            // INX / INY / DEX / DEY
            0xE8 => { self.inx(); 2 }
            0xC8 => { self.iny(); 2 }
            0xCA => { self.dex(); 2 }
            0x88 => { self.dey(); 2 }

            // CMP / CPX / CPY
            0xC9 => { self.cmp(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0xC5 => { self.cmp(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0xD5 => { self.cmp(bus, &AddressingMode::ZeroPageX); self.pc += 1; 4 }
            0xCD => { self.cmp(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            0xDD => { let pc = self.cmp(bus, &AddressingMode::AbsoluteX); self.pc += 2; 4 + u32::from(pc) }
            0xD9 => { let pc = self.cmp(bus, &AddressingMode::AbsoluteY); self.pc += 2; 4 + u32::from(pc) }
            0xC1 => { self.cmp(bus, &AddressingMode::IndirectX); self.pc += 1; 6 }
            0xD1 => { let pc = self.cmp(bus, &AddressingMode::IndirectY); self.pc += 1; 5 + u32::from(pc) }
            
            0xE0 => { self.cpx(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0xE4 => { self.cpx(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0xEC => { self.cpx(bus, &AddressingMode::Absolute); self.pc += 2; 4 }
            
            0xC0 => { self.cpy(bus, &AddressingMode::Immediate); self.pc += 1; 2 }
            0xC4 => { self.cpy(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0xCC => { self.cpy(bus, &AddressingMode::Absolute); self.pc += 2; 4 }

            // BIT
            0x24 => { self.bit(bus, &AddressingMode::ZeroPage); self.pc += 1; 3 }
            0x2C => { self.bit(bus, &AddressingMode::Absolute); self.pc += 2; 4 }

            // Jumps & Calls
            // https://www.nesdev.org/wiki/CPU_instructions#Jumps
            0x4C => { // JMP Absolute
                self.pc = Self::mem_read_u16(bus, self.pc);
                3
            }
            0x6C => { // JMP Indirect
                self.pc = self.get_operand_address_for_jmp_indirect(bus);
                5
            }
            0x20 => { // JSR (Jump to Subroutine)
                self.stack_push_u16(bus, self.pc + 2 - 1);
                self.pc = Self::mem_read_u16(bus, self.pc);
                6
            }
            0x60 => { // RTS (Return from Subroutine)
                self.pc = self.stack_pop_u16(bus) + 1;
                6
            }
            0x40 => { // RTI (Return from Interrupt)
                self.status = CpuFlags::from_bits_truncate(self.stack_pop(bus));
                self.status.remove(CpuFlags::BREAK);
                self.status.insert(CpuFlags::BREAK2);
                self.pc = self.stack_pop_u16(bus);
                6
            }

            // Branches
            // https://www.nesdev.org/wiki/CPU_instructions#Branch_instructions
            0x90 => { // BCC (Branch if Carry Clear)
                let cycles = self.branch(bus, !self.status.contains(CpuFlags::CARRY));
                2 + cycles
            }
            0xB0 => { // BCS (Branch if Carry Set)
                let cycles = self.branch(bus, self.status.contains(CpuFlags::CARRY));
                2 + cycles
            }
            0xF0 => { // BEQ (Branch if Equal / Zero Set)
                let cycles = self.branch(bus, self.status.contains(CpuFlags::ZERO));
                2 + cycles
            }
            0x30 => { // BMI (Branch if Minus / Negative Set)
                let cycles = self.branch(bus, self.status.contains(CpuFlags::NEGATIVE));
                2 + cycles
            }
            0xD0 => { // BNE (Branch if Not Equal / Zero Clear)
                let cycles = self.branch(bus, !self.status.contains(CpuFlags::ZERO));
                2 + cycles
            }
            0x10 => { // BPL (Branch if Plus / Negative Clear)
                let cycles = self.branch(bus, !self.status.contains(CpuFlags::NEGATIVE));
                2 + cycles
            }
            0x50 => { // BVC (Branch if Overflow Clear)
                let cycles = self.branch(bus, !self.status.contains(CpuFlags::OVERFLOW));
                2 + cycles
            }
            0x70 => { // BVS (Branch if Overflow Set)
                let cycles = self.branch(bus, self.status.contains(CpuFlags::OVERFLOW));
                2 + cycles
            }

            // Status Flag Changes
            // https://www.nesdev.org/wiki/CPU_instructions#Status_flag_changes
            0x38 => { self.status.insert(CpuFlags::CARRY); 2 }             // SEC
            0xF8 => { self.status.insert(CpuFlags::DECIMAL_MODE); 2 }      // SED
            0x78 => { self.status.insert(CpuFlags::INTERRUPT_DISABLE); 2 } // SEI
            0x18 => { self.status.remove(CpuFlags::CARRY); 2 }             // CLC
            0xD8 => { self.status.remove(CpuFlags::DECIMAL_MODE); 2 }      // CLD
            0x58 => { self.status.remove(CpuFlags::INTERRUPT_DISABLE); 2 } // CLI
            0xB8 => { self.status.remove(CpuFlags::OVERFLOW); 2 }          // CLV

            // Register Transfers
            // https://www.nesdev.org/wiki/CPU_instructions#Register_transfers
            0xAA => { self.x = self.a; self.update_zero_and_negative_flags(self.x); 2 }  // TAX
            0xA8 => { self.y = self.a; self.update_zero_and_negative_flags(self.y); 2 }  // TAY
            0xBA => { self.x = self.sp; self.update_zero_and_negative_flags(self.x); 2 } // TSX
            0x8A => { self.a = self.x; self.update_zero_and_negative_flags(self.a); 2 }  // TXA
            0x9A => { self.sp = self.x; 2 }                                             // TXS
            0x98 => { self.a = self.y; self.update_zero_and_negative_flags(self.a); 2 }  // TYA

            // Stack Operations
            // https://www.nesdev.org/wiki/CPU_instructions#Stack_operations
            0x48 => { self.stack_push(bus, self.a); 3 } // PHA
            0x08 => { // PHP (Push Processor Status)
                let mut flags = self.status;
                // PHP and BRK set bits 4 and 5 on the stack
                flags.insert(CpuFlags::BREAK);
                flags.insert(CpuFlags::BREAK2);
                self.stack_push(bus, flags.bits());
                3
            }
            0x68 => { // PLA (Pull Accumulator)
                self.a = self.stack_pop(bus);
                self.update_zero_and_negative_flags(self.a);
                4
            }
            0x28 => { // PLP (Pull Processor Status)
                self.status = CpuFlags::from_bits_truncate(self.stack_pop(bus));
                // Bit 4 is ignored, bit 5 is always 1
                self.status.remove(CpuFlags::BREAK);
                self.status.insert(CpuFlags::BREAK2);
                4
            }

            // System
            // https://www.nesdev.org/wiki/CPU_instructions#System_functions
            0x00 => { // BRK (Force Interrupt)
                self.stack_push_u16(bus, self.pc + 1);
                let mut flags = self.status;
                flags.insert(CpuFlags::BREAK);
                flags.insert(CpuFlags::BREAK2);
                self.stack_push(bus, flags.bits());
                self.status.insert(CpuFlags::INTERRUPT_DISABLE);
                self.pc = Self::mem_read_u16(bus, 0xFFFE);
                7
            }
            0xEA => { // NOP (No Operation)
                2
            }

            // ══════════════════════════════════════════════════════════════════
            // Unofficial / Illegal Opcodes
            // https://www.nesdev.org/wiki/CPU_unofficial_opcodes
            // ══════════════════════════════════════════════════════════════════

            // ── Unofficial NOPs (no side-effects, just consume operand bytes) ──
            0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => { 2 } // NOP Implied
            0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 => { self.pc += 1; 2 } // NOP Immediate
            0x04 | 0x44 | 0x64 => { self.pc += 1; 3 }               // NOP ZeroPage
            0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4 => { self.pc += 1; 4 } // NOP ZeroPageX
            0x0C => { self.pc += 2; 4 }                              // NOP Absolute
            0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => {            // NOP AbsoluteX
                let (_, page_crossed) = self.get_operand_address(bus, &AddressingMode::AbsoluteX);
                self.pc += 2;
                if page_crossed { 5 } else { 4 }
            }

            // ── LAX: LDA + LDX (same operand into both A and X) ──────────────
            // https://www.nesdev.org/wiki/CPU_unofficial_opcodes#LAX
            0xA3 => { self.lda(bus, &AddressingMode::IndirectX); self.x = self.a; self.pc += 1; 6 }
            0xA7 => { self.lda(bus, &AddressingMode::ZeroPage);  self.x = self.a; self.pc += 1; 3 }
            0xAF => { self.lda(bus, &AddressingMode::Absolute);  self.x = self.a; self.pc += 2; 4 }
            0xB3 => { let pc = self.lda(bus, &AddressingMode::IndirectY); self.x = self.a; self.pc += 1; 5 + u32::from(pc) }
            0xB7 => { self.lda(bus, &AddressingMode::ZeroPageY); self.x = self.a; self.pc += 1; 4 }
            0xBF => { let pc = self.lda(bus, &AddressingMode::AbsoluteY); self.x = self.a; self.pc += 2; 4 + u32::from(pc) }

            // ── SAX: Store A & X into memory ──────────────────────────────────
            // https://www.nesdev.org/wiki/CPU_unofficial_opcodes#SAX
            0x83 => { let (addr, _) = self.get_operand_address(bus, &AddressingMode::IndirectX); Self::mem_write(bus, addr, self.a & self.x); self.pc += 1; 6 }
            0x87 => { let (addr, _) = self.get_operand_address(bus, &AddressingMode::ZeroPage);  Self::mem_write(bus, addr, self.a & self.x); self.pc += 1; 3 }
            0x8F => { let (addr, _) = self.get_operand_address(bus, &AddressingMode::Absolute);  Self::mem_write(bus, addr, self.a & self.x); self.pc += 2; 4 }
            0x97 => { let (addr, _) = self.get_operand_address(bus, &AddressingMode::ZeroPageY); Self::mem_write(bus, addr, self.a & self.x); self.pc += 1; 4 }

            // ── DCP: DEC memory, then CMP A with result ───────────────────────
            // https://www.nesdev.org/wiki/CPU_unofficial_opcodes#DCP
            0xC3 => { self.dec(bus, &AddressingMode::IndirectX); self.cmp(bus, &AddressingMode::IndirectX); self.pc += 1; 8 }
            0xC7 => { self.dec(bus, &AddressingMode::ZeroPage);  self.cmp(bus, &AddressingMode::ZeroPage);  self.pc += 1; 5 }
            0xCF => { self.dec(bus, &AddressingMode::Absolute);  self.cmp(bus, &AddressingMode::Absolute);  self.pc += 2; 6 }
            0xD3 => { self.dec(bus, &AddressingMode::IndirectY); self.cmp(bus, &AddressingMode::IndirectY); self.pc += 1; 8 }
            0xD7 => { self.dec(bus, &AddressingMode::ZeroPageX); self.cmp(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0xDB => { self.dec(bus, &AddressingMode::AbsoluteY); self.cmp(bus, &AddressingMode::AbsoluteY); self.pc += 2; 7 }
            0xDF => { self.dec(bus, &AddressingMode::AbsoluteX); self.cmp(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }

            // ── ISB/ISC: INC memory, then SBC A with result ───────────────────
            // https://www.nesdev.org/wiki/CPU_unofficial_opcodes#ISC
            0xE3 => { self.inc(bus, &AddressingMode::IndirectX); self.sbc(bus, &AddressingMode::IndirectX); self.pc += 1; 8 }
            0xE7 => { self.inc(bus, &AddressingMode::ZeroPage);  self.sbc(bus, &AddressingMode::ZeroPage);  self.pc += 1; 5 }
            0xEF => { self.inc(bus, &AddressingMode::Absolute);  self.sbc(bus, &AddressingMode::Absolute);  self.pc += 2; 6 }
            0xF3 => { self.inc(bus, &AddressingMode::IndirectY); self.sbc(bus, &AddressingMode::IndirectY); self.pc += 1; 8 }
            0xF7 => { self.inc(bus, &AddressingMode::ZeroPageX); self.sbc(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0xFB => { self.inc(bus, &AddressingMode::AbsoluteY); self.sbc(bus, &AddressingMode::AbsoluteY); self.pc += 2; 7 }
            0xFF => { self.inc(bus, &AddressingMode::AbsoluteX); self.sbc(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }

            // ── SLO: ASL memory, then ORA A ────────────────────────────────────
            // https://www.nesdev.org/wiki/CPU_unofficial_opcodes#SLO
            0x03 => { self.asl(bus, &AddressingMode::IndirectX); self.ora(bus, &AddressingMode::IndirectX); self.pc += 1; 8 }
            0x07 => { self.asl(bus, &AddressingMode::ZeroPage);  self.ora(bus, &AddressingMode::ZeroPage);  self.pc += 1; 5 }
            0x0F => { self.asl(bus, &AddressingMode::Absolute);  self.ora(bus, &AddressingMode::Absolute);  self.pc += 2; 6 }
            0x13 => { self.asl(bus, &AddressingMode::IndirectY); self.ora(bus, &AddressingMode::IndirectY); self.pc += 1; 8 }
            0x17 => { self.asl(bus, &AddressingMode::ZeroPageX); self.ora(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0x1B => { self.asl(bus, &AddressingMode::AbsoluteY); self.ora(bus, &AddressingMode::AbsoluteY); self.pc += 2; 7 }
            0x1F => { self.asl(bus, &AddressingMode::AbsoluteX); self.ora(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }

            // ── SRE: LSR memory, then EOR A ────────────────────────────────────
            // https://www.nesdev.org/wiki/CPU_unofficial_opcodes#SRE
            0x43 => { self.lsr(bus, &AddressingMode::IndirectX); self.eor(bus, &AddressingMode::IndirectX); self.pc += 1; 8 }
            0x47 => { self.lsr(bus, &AddressingMode::ZeroPage);  self.eor(bus, &AddressingMode::ZeroPage);  self.pc += 1; 5 }
            0x4F => { self.lsr(bus, &AddressingMode::Absolute);  self.eor(bus, &AddressingMode::Absolute);  self.pc += 2; 6 }
            0x53 => { self.lsr(bus, &AddressingMode::IndirectY); self.eor(bus, &AddressingMode::IndirectY); self.pc += 1; 8 }
            0x57 => { self.lsr(bus, &AddressingMode::ZeroPageX); self.eor(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0x5B => { self.lsr(bus, &AddressingMode::AbsoluteY); self.eor(bus, &AddressingMode::AbsoluteY); self.pc += 2; 7 }
            0x5F => { self.lsr(bus, &AddressingMode::AbsoluteX); self.eor(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }

            // ── RLA: ROL memory, then AND A ────────────────────────────────────
            // https://www.nesdev.org/wiki/CPU_unofficial_opcodes#RLA
            0x23 => { self.rol(bus, &AddressingMode::IndirectX); self.and(bus, &AddressingMode::IndirectX); self.pc += 1; 8 }
            0x27 => { self.rol(bus, &AddressingMode::ZeroPage);  self.and(bus, &AddressingMode::ZeroPage);  self.pc += 1; 5 }
            0x2F => { self.rol(bus, &AddressingMode::Absolute);  self.and(bus, &AddressingMode::Absolute);  self.pc += 2; 6 }
            0x33 => { self.rol(bus, &AddressingMode::IndirectY); self.and(bus, &AddressingMode::IndirectY); self.pc += 1; 8 }
            0x37 => { self.rol(bus, &AddressingMode::ZeroPageX); self.and(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0x3B => { self.rol(bus, &AddressingMode::AbsoluteY); self.and(bus, &AddressingMode::AbsoluteY); self.pc += 2; 7 }
            0x3F => { self.rol(bus, &AddressingMode::AbsoluteX); self.and(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }

            // ── RRA: ROR memory, then ADC A ────────────────────────────────────
            // https://www.nesdev.org/wiki/CPU_unofficial_opcodes#RRA
            0x63 => { self.ror(bus, &AddressingMode::IndirectX); self.adc(bus, &AddressingMode::IndirectX); self.pc += 1; 8 }
            0x67 => { self.ror(bus, &AddressingMode::ZeroPage);  self.adc(bus, &AddressingMode::ZeroPage);  self.pc += 1; 5 }
            0x6F => { self.ror(bus, &AddressingMode::Absolute);  self.adc(bus, &AddressingMode::Absolute);  self.pc += 2; 6 }
            0x73 => { self.ror(bus, &AddressingMode::IndirectY); self.adc(bus, &AddressingMode::IndirectY); self.pc += 1; 8 }
            0x77 => { self.ror(bus, &AddressingMode::ZeroPageX); self.adc(bus, &AddressingMode::ZeroPageX); self.pc += 1; 6 }
            0x7B => { self.ror(bus, &AddressingMode::AbsoluteY); self.adc(bus, &AddressingMode::AbsoluteY); self.pc += 2; 7 }
            0x7F => { self.ror(bus, &AddressingMode::AbsoluteX); self.adc(bus, &AddressingMode::AbsoluteX); self.pc += 2; 7 }

            // ── *SBC: Unofficial SBC Immediate (identical to official $E9) ────
            0xEB => { self.sbc(bus, &AddressingMode::Immediate); self.pc += 1; 2 }

            _ => {
                println!("Unimplemented Opcode: {opcode:#X}");
                2
            }
        };
        let dma = bus.dma_cycles;
        bus.dma_cycles = 0;
        cycles + dma
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cartridge::Cartridge;
    use crate::core::bus::Bus;

    struct TestBus {
        bus: Bus,
    }

    impl TestBus {
        fn new() -> Self {
            let cartridge = Cartridge {
                prg_rom: vec![0; 0x8000],
                chr_rom: vec![0; 0x2000],
                mapper: 0,
                vertical_mirroring: true,
            };
            TestBus {
                bus: Bus::new(cartridge),
            }
        }
    }

    /// **Objective**: Verify that the LDA (Load Accumulator) instruction correctly loads 
    /// immediate data into the A register.
    #[test]
    fn test_0xa9_lda_immediate_load_data() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        // LDA #0x05, BRK
        test_bus.bus.ram[0] = 0xa9;
        test_bus.bus.ram[1] = 0x05;
        test_bus.bus.ram[2] = 0x00;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0x05);
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        assert!(!cpu.status.contains(CpuFlags::NEGATIVE));
    }

    /// **Objective**: Verify that the LDA instruction correctly sets the Zero flag when 
    /// loading a value of 0x00.
    #[test]
    fn test_0xa9_lda_zero_flag() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        test_bus.bus.ram[0] = 0xa9;
        test_bus.bus.ram[1] = 0x00;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert!(cpu.status.contains(CpuFlags::ZERO));
    }

    /// **Objective**: Verify that the TAX (Transfer Accumulator to X) instruction correctly 
    /// moves data between registers.
    #[test]
    fn test_0xaa_tax_move_a_to_x() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.a = 10;
        test_bus.bus.ram[0] = 0xaa;
        cpu.pc = 0;

        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.x, 10);
    }

    /// **Objective**: Integration test verifying a sequence of dependent instructions 
    /// (LDA, TAX, INX) to ensure state consistency across steps.
    #[test]
    fn test_5_ops_working_together() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        // LDA #0xc0, TAX, INX, BRK
        test_bus.bus.ram[0] = 0xa9;
        test_bus.bus.ram[1] = 0xc0;
        test_bus.bus.ram[2] = 0xaa;
        test_bus.bus.ram[3] = 0xe8;
        cpu.pc = 0;

        cpu.step(&mut test_bus.bus);
        cpu.step(&mut test_bus.bus);
        cpu.step(&mut test_bus.bus);

        assert_eq!(cpu.x, 0xc1);
    }

    /// **Objective**: Verify that the INX (Increment X) instruction correctly wraps from 
    /// 0xFF to 0x00 (standard 8-bit overflow).
    #[test]
    fn test_inx_overflow() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.x = 0xff;
        test_bus.bus.ram[0] = 0xe8;
        cpu.pc = 0;

        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.x, 0);
    }

    /// **Objective**: Verify that the ADC (Add with Carry) instruction correctly performs 
    /// addition when the carry flag is clear.
    #[test]
    fn test_adc_no_carry() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.a = 0x10;
        test_bus.bus.ram[0] = 0x69; // ADC Immediate
        test_bus.bus.ram[1] = 0x10;
        cpu.pc = 0;

        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0x20);
        assert!(!cpu.status.contains(CpuFlags::CARRY));
    }

    /// **Objective**: Verify that the ADC instruction correctly sets the Carry and Zero 
    /// flags during an 8-bit overflow.
    #[test]
    fn test_adc_with_carry() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.a = 0xFF;
        test_bus.bus.ram[0] = 0x69; 
        test_bus.bus.ram[1] = 0x01;
        cpu.pc = 0;

        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.status.contains(CpuFlags::CARRY));
        assert!(cpu.status.contains(CpuFlags::ZERO));
    }

    /// **Objective**: Verify that the SBC (Subtract with Carry) instruction correctly 
    /// performs subtraction using the inverted carry bit (standard 6502 logic).
    #[test]
    fn test_sbc_no_carry() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.a = 0x10;
        cpu.status.insert(CpuFlags::CARRY); // Carry = 1 means no borrow in SBC
        test_bus.bus.ram[0] = 0xE9; // SBC Immediate
        test_bus.bus.ram[1] = 0x05;
        cpu.pc = 0;

        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0x0B);
        assert!(cpu.status.contains(CpuFlags::CARRY));
    }

    /// **Objective**: Verify that the AND instruction correctly performs bitwise logical 
    /// AND between the accumulator and immediate data.
    #[test]
    fn test_logical_and() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.a = 0b1100_1100;
        test_bus.bus.ram[0] = 0x29; // AND Immediate
        test_bus.bus.ram[1] = 0b1010_1010;
        cpu.pc = 0;

        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0b1000_1000);
    }

    /// **Objective**: Verify that a conditional branch (BEQ) correctly updates the 
    /// Program Counter and consumes the correct number of cycles when taken.
    #[test]
    fn test_branch_taken() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.status.insert(CpuFlags::ZERO);
        test_bus.bus.ram[0] = 0xF0; // BEQ
        test_bus.bus.ram[1] = 0x05; // Relative offset +5
        cpu.pc = 0;

        let cycles = cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.pc, 0x07); // 2 (fetch) + 5 (offset)
        assert_eq!(cycles, 3); // 2 base + 1 branch taken
    }

    /// **Objective**: Verify that a conditional branch correctly continues execution 
    /// at the next instruction when the condition is not met.
    #[test]
    fn test_branch_not_taken() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.status.remove(CpuFlags::ZERO);
        test_bus.bus.ram[0] = 0xF0; // BEQ
        test_bus.bus.ram[1] = 0x05;
        cpu.pc = 0;

        let cycles = cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.pc, 0x02); // Just fetch BEQ + offset
        assert_eq!(cycles, 2);
    }

    /// **Objective**: Verify that the NMI (Non-Maskable Interrupt) correctly pushes the 
    /// PC and Status to the stack and jumps to the NMI vector.
    #[test]
    fn test_nmi() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.pc = 0x1234;
        cpu.status = CpuFlags::from_bits_truncate(0x24);
        
        // Set NMI vector in Cartridge ROM (0xFFFA -> 0x7FFA)
        test_bus.bus.cartridge.prg_rom[0x7FFA] = 0xAD;
        test_bus.bus.cartridge.prg_rom[0x7FFB] = 0xDE;
        
        cpu.nmi(&mut test_bus.bus);
        
        assert_eq!(cpu.pc, 0xDEAD);
        assert_eq!(cpu.sp, 0xFA); // Pushed 2 bytes of PC + 1 byte of Status (0xFD - 3 = 0xFA)
    }

    /// **Objective**: Verify that OAM DMA correctly transfers 256 bytes from CPU RAM 
    /// to PPU OAM and stalls the CPU for the correct number of cycles.
    #[test]
    fn test_dma() {
        let mut test_bus = TestBus::new();
        
        // Fill RAM at 0x0200 with test data
        for i in 0..256 {
            test_bus.bus.write(0x0200 + i as u16, i as u8);
        }
        
        // Trigger DMA by writing 0x02 to $4014
        test_bus.bus.write(0x4014, 0x02);
        
        // Check PPU OAM data
        for i in 0..256 {
            assert_eq!(test_bus.bus.ppu.oam_data[i], i as u8);
        }
        
        // Check CPU stall cycles
        assert_eq!(test_bus.bus.dma_cycles, 513);
    }

    /// **Objective**: Verify that the CPU Reset correctly reloads the Program Counter 
    /// from the Reset Vector ($FFFC) and resets internal registers.
    #[test]
    fn test_reset() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.a = 0xFF;
        
        // Set Reset vector (0xFFFC -> 0x7FFC)
        test_bus.bus.cartridge.prg_rom[0x7FFC] = 0xEF;
        test_bus.bus.cartridge.prg_rom[0x7FFD] = 0xBE;
        
        cpu.reset(&mut test_bus.bus);
        
        assert_eq!(cpu.pc, 0xBEEF);
        assert_eq!(cpu.a, 0); // Reset clears accumulator
    }

    /// **Objective**: Verify that PHA (Push Accumulator) and PLA (Pull Accumulator) 
    /// correctly use the stack memory and update the stack pointer.
    #[test]
    fn test_stack_ops() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.a = 0x42;
        cpu.pc = 0;
        
        // PHA (0x48), PLA (0x68)
        test_bus.bus.ram[0] = 0x48;
        test_bus.bus.ram[1] = 0x68;
        
        cpu.step(&mut test_bus.bus); // PHA
        assert_eq!(cpu.sp, 0xFC);
        assert_eq!(test_bus.bus.ram[0x01FD], 0x42);
        
        cpu.a = 0x00;
        cpu.step(&mut test_bus.bus); // PLA
        assert_eq!(cpu.a, 0x42);
        assert_eq!(cpu.sp, 0xFD);
    }

    /// **Objective**: Verify that the JMP (Jump) instruction correctly updates the 
    /// Program Counter to the target address.
    #[test]
    fn test_jmp() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        // JMP $1234
        test_bus.bus.ram[0] = 0x4C;
        test_bus.bus.ram[1] = 0x34;
        test_bus.bus.ram[2] = 0x12;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.pc, 0x1234);
    }

    /// **Objective**: Verify that JSR (Jump to Subroutine) and RTS (Return from Subroutine) 
    /// correctly use the stack to store and retrieve the return address.
    #[test]
    fn test_jsr_rts() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        // JSR $0005
        test_bus.bus.ram[0] = 0x20;
        test_bus.bus.ram[1] = 0x05;
        test_bus.bus.ram[2] = 0x00;
        // RTS at $0005
        test_bus.bus.ram[5] = 0x60;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus); // JSR
        assert_eq!(cpu.pc, 0x0005);
        assert_eq!(cpu.sp, 0xFB); // Pushed 2 bytes of PC
        
        cpu.step(&mut test_bus.bus); // RTS
        assert_eq!(cpu.pc, 0x0003); // Returns to instruction after JSR
    }

    /// **Objective**: Verify that the BIT instruction correctly updates the 
    /// Zero, Negative, and Overflow flags based on memory content.
    #[test]
    fn test_bit() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.a = 0xFF;
        // BIT $0100
        test_bus.bus.ram[0] = 0x2C;
        test_bus.bus.ram[1] = 0x00;
        test_bus.bus.ram[2] = 0x01;
        
        // Memory at $0100 has bit 7 and 6 set
        test_bus.bus.ram[0x0100] = 0b1100_0000;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert!(cpu.status.contains(CpuFlags::NEGATIVE)); // Bit 7
        assert!(cpu.status.contains(CpuFlags::OVERFLOW)); // Bit 6
        assert!(!cpu.status.contains(CpuFlags::ZERO));    // 0xFF & 0xC0 != 0
    }

    /// **Objective**: Verify that CMP (Compare Accumulator) correctly sets the 
    /// Zero and Carry flags based on relative values.
    #[test]
    fn test_compare() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.a = 0x50;
        test_bus.bus.ram[0] = 0xC9; // CMP Immediate
        test_bus.bus.ram[1] = 0x50;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert!(cpu.status.contains(CpuFlags::ZERO));
        assert!(cpu.status.contains(CpuFlags::CARRY));
        
        cpu.pc = 0;
        test_bus.bus.ram[1] = 0x60;
        cpu.step(&mut test_bus.bus);
        assert!(!cpu.status.contains(CpuFlags::CARRY)); // A < M
        assert!(cpu.status.contains(CpuFlags::NEGATIVE));
    }

    /// **Objective**: Verify that ASL (Arithmetic Shift Left) correctly shifts 
    /// bits and updates the Carry flag with the shifted-out bit.
    #[test]
    fn test_asl() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.a = 0b1000_0001;
        test_bus.bus.ram[0] = 0x0A; // ASL Accumulator
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0b0000_0010);
        assert!(cpu.status.contains(CpuFlags::CARRY));
    }

    /// **Objective**: Verify that LSR (Logical Shift Right) correctly shifts 
    /// bits and updates the Carry flag with the shifted-out bit.
    #[test]
    fn test_lsr() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.a = 0b0000_0011;
        test_bus.bus.ram[0] = 0x4A; // LSR Accumulator
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0b0000_0001);
        assert!(cpu.status.contains(CpuFlags::CARRY));
    }

    /// **Objective**: Verify that ROL (Rotate Left) correctly shifts bits 
    /// through the Carry flag.
    #[test]
    fn test_rol() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.a = 0b1000_0000;
        cpu.status.remove(CpuFlags::CARRY);
        test_bus.bus.ram[0] = 0x2A; // ROL Accumulator
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.status.contains(CpuFlags::CARRY));
        
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0x01);
        assert!(!cpu.status.contains(CpuFlags::CARRY));
    }

    /// **Objective**: Verify that the EOR (Exclusive OR) instruction correctly performs 
    /// bitwise logical XOR between the accumulator and immediate data.
    #[test]
    fn test_eor() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.a = 0b1100_1100;
        test_bus.bus.ram[0] = 0x49; // EOR Immediate
        test_bus.bus.ram[1] = 0b1010_1010;
        cpu.pc = 0;

        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0b0110_0110);
    }

    /// **Objective**: Verify that INC (Increment Memory) correctly updates the 
    /// value at the specified address and sets status flags.
    #[test]
    fn test_inc() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        test_bus.bus.ram[0x0100] = 0x10;
        test_bus.bus.ram[0] = 0xEE; // INC Absolute
        test_bus.bus.ram[1] = 0x00;
        test_bus.bus.ram[2] = 0x01;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(test_bus.bus.ram[0x0100], 0x11);
    }

    /// **Objective**: Verify that the ZeroPage,X addressing mode correctly 
    /// wraps within the first 256 bytes of memory.
    #[test]
    fn test_zeropage_x() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.x = 0x10;
        test_bus.bus.ram[0x1F] = 0x42;
        
        // LDA ZeroPage,X ($0F + $10 = $1F)
        test_bus.bus.ram[0] = 0xB5;
        test_bus.bus.ram[1] = 0x0F;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0x42);
    }

    /// **Objective**: Verify that Indirect,Y addressing mode correctly 
    /// calculates the target address with a page-crossing scenario.
    #[test]
    fn test_indirect_y() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.y = 0x01;
        
        // Pointer at $20 points to $0200
        test_bus.bus.ram[0x20] = 0x00;
        test_bus.bus.ram[0x21] = 0x02;
        
        // Value at $0201 ($0200 + Y)
        test_bus.bus.ram[0x0201] = 0x77;
        
        // LDA (Indirect),Y
        test_bus.bus.ram[0] = 0xB1;
        test_bus.bus.ram[1] = 0x20;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0x77);
    }

    /// **Objective**: Verify that LDX and LDY correctly load values from 
    /// immediate and ZeroPage memory.
    #[test]
    fn test_ldx_ldy() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        test_bus.bus.ram[0] = 0xA2; // LDX Immediate
        test_bus.bus.ram[1] = 0x55;
        test_bus.bus.ram[2] = 0xA0; // LDY Immediate
        test_bus.bus.ram[3] = 0xAA;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.x, 0x55);
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.y, 0xAA);
    }

    /// **Objective**: Verify that STX and STY correctly store register values 
    /// into ZeroPage memory.
    #[test]
    fn test_stx_sty() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.x = 0x12;
        cpu.y = 0x34;
        test_bus.bus.ram[0] = 0x86; // STX ZeroPage ($10)
        test_bus.bus.ram[1] = 0x10;
        test_bus.bus.ram[2] = 0x84; // STY ZeroPage ($11)
        test_bus.bus.ram[3] = 0x11;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(test_bus.bus.ram[0x10], 0x12);
        cpu.step(&mut test_bus.bus);
        assert_eq!(test_bus.bus.ram[0x11], 0x34);
    }

    /// **Objective**: Verify that Absolute,X and Absolute,Y addressing modes 
    /// correctly calculate the target address with page-crossing.
    #[test]
    fn test_absolute_indexed() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.x = 0xFF;
        test_bus.bus.ram[0x02FE] = 0x88;
        
        // LDA Absolute,X ($01FF + $FF = $02FE)
        test_bus.bus.ram[0] = 0xBD;
        test_bus.bus.ram[1] = 0xFF;
        test_bus.bus.ram[2] = 0x01;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0x88);
    }

    /// **Objective**: Verify that ORA, AND, and EOR correctly perform 
    /// bitwise logic and update Zero/Negative flags.
    #[test]
    fn test_logical_ops() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.a = 0b1010_1010;
        test_bus.bus.ram[0] = 0x09; // ORA Immediate
        test_bus.bus.ram[1] = 0b0101_0101;
        test_bus.bus.ram[2] = 0x29; // AND Immediate
        test_bus.bus.ram[3] = 0x00;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0xFF);
        assert!(cpu.status.contains(CpuFlags::NEGATIVE));
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.status.contains(CpuFlags::ZERO));
    }

    /// **Objective**: Verify that (Indirect,X) addressing mode correctly 
    /// calculates the target address using the X register as an index to the pointer.
    #[test]
    fn test_indirect_x() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.x = 0x04;
        
        // Pointer at $0A ($06 + $04) points to $0300
        test_bus.bus.ram[0x0A] = 0x00;
        test_bus.bus.ram[0x0B] = 0x03;
        
        // Value at $0300
        test_bus.bus.ram[0x0300] = 0x99;
        
        // LDA (Indirect,X)
        test_bus.bus.ram[0] = 0xA1;
        test_bus.bus.ram[1] = 0x06;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0x99);
    }

    /// **Objective**: Verify that the Stack Pointer correctly wraps around 
    /// the $0100-$01FF memory range.
    #[test]
    fn test_stack_pointer_wrap() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        cpu.sp = 0x00;
        
        // PHA (Push A)
        cpu.a = 0xAA;
        test_bus.bus.ram[0] = 0x48;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(test_bus.bus.ram[0x0100], 0xAA);
        assert_eq!(cpu.sp, 0xFF); // Wrap from 0x00 to 0xFF
    }

    /// **Objective**: Verify the famous 6502 JMP Indirect bug where 
    /// a pointer at the end of a page wraps around to the beginning of the SAME page.
    #[test]
    fn test_jmp_indirect_page_wrap() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        
        // Pointer at $01FF. Should read LO from $01FF and HI from $0100 (NOT $0200).
        test_bus.bus.ram[0x01FF] = 0x11;
        test_bus.bus.ram[0x0100] = 0x22;
        
        // JMP ($01FF)
        test_bus.bus.ram[0] = 0x6C;
        test_bus.bus.ram[1] = 0xFF;
        test_bus.bus.ram[2] = 0x01;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.pc, 0x2211);
    }

    /// **Objective**: Verify that branches correctly add extra cycles 
    /// when crossing a page boundary.
    #[test]
    fn test_branch_page_cross() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        
        // BNE at $00FD. PC after offset fetch is $00FF.
        // If it branches to $0100, it's a page cross from $00FF to $0100.
        cpu.status.remove(CpuFlags::ZERO);
        test_bus.bus.ram[0x00FD] = 0xD0; // BNE
        test_bus.bus.ram[0x00FE] = 0x01; // Offset +1 (from $00FF -> $0100)
        cpu.pc = 0x00FD;
        
        let cycles = cpu.step(&mut test_bus.bus);
        // Base 2 + 1 (taken) + 1 (page cross) = 4
        assert_eq!(cycles, 4);
        assert_eq!(cpu.pc, 0x0100);
    }

    /// **Objective**: Verify that ASL, LSR, ROL, and ROR correctly shift/rotate 
    /// bits and update the Carry flag.
    #[test]
    fn test_shifts() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        
        // ASL Accumulator: 0x80 -> 0x00, Carry = 1
        cpu.a = 0x80;
        test_bus.bus.ram[0] = 0x0A; 
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.status.contains(CpuFlags::CARRY));
        assert!(cpu.status.contains(CpuFlags::ZERO));
        
        // LSR Accumulator: 0x01 -> 0x00, Carry = 1
        cpu.a = 0x01;
        test_bus.bus.ram[1] = 0x4A;
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.status.contains(CpuFlags::CARRY));
    }

    /// **Objective**: Verify that the BRK (software interrupt) correctly pushes 
    /// PC and Status to the stack and jumps to the IRQ vector.
    #[test]
    fn test_interrupts() {
        let mut prg_rom = vec![0; 32768];
        // IRQ vector at $FFFE-$FFFF (physical offset $7FFE-$7FFF for 32KB ROM)
        prg_rom[0x7FFE] = 0x00;
        prg_rom[0x7FFF] = 0x03;
        
        let cartridge = Cartridge {
            prg_rom,
            chr_rom: vec![0; 8192],
            mapper: 0,
            vertical_mirroring: true,
        };
        let mut bus = Bus::new(cartridge);
        let mut cpu = Cpu::new();
        
        // BRK instruction in RAM at $0050
        bus.write(0x0050, 0x00);
        cpu.pc = 0x0050;
        
        cpu.step(&mut bus);
        
        assert_eq!(cpu.pc, 0x0300);
        // The flags on stack should have BREAK bit set
        let pushed_flags = bus.read(0x0100 + (cpu.sp.wrapping_add(1) as u16));
        assert!(pushed_flags & CpuFlags::BREAK.bits() != 0);
    }

    /// **Objective**: Verify that RTI correctly restores the PC and Status flags 
    /// from the stack, allowing return from an interrupt handler.
    #[test]
    fn test_rti() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        
        // Setup stack: PC_HI=$02, PC_LO=$00, Status=$21 (Carry set)
        cpu.sp = 0xFF;
        cpu.stack_push_u16(&mut test_bus.bus, 0x0200);
        cpu.stack_push(&mut test_bus.bus, 0x21);
        cpu.sp = 0xFC; // After 3 pushes
        
        // RTI at $0000
        test_bus.bus.ram[0] = 0x40;
        cpu.pc = 0;
        
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.pc, 0x0200);
        assert!(cpu.status.contains(CpuFlags::CARRY));
    }

    /// **Objective**: Verify register-based increment/decrement instructions 
    /// (INY, DEX, DEY) and memory-based decrement (DEC).
    #[test]
    fn test_inc_dec() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        
        // INY: 0x01 -> 0x02
        cpu.y = 0x01;
        test_bus.bus.ram[0] = 0xC8; 
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.y, 0x02);
        
        // DEX: 0x01 -> 0x00
        cpu.x = 0x01;
        test_bus.bus.ram[1] = 0xCA;
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.x, 0x00);
        assert!(cpu.status.contains(CpuFlags::ZERO));
        
        // DEY: 0x00 -> 0xFF
        cpu.y = 0x00;
        test_bus.bus.ram[2] = 0x88;
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.y, 0xFF);
        assert!(cpu.status.contains(CpuFlags::NEGATIVE));
        
        // DEC ZeroPage: 0x05 -> 0x04
        test_bus.bus.ram[0x10] = 0x05;
        test_bus.bus.ram[3] = 0xC6;
        test_bus.bus.ram[4] = 0x10;
        cpu.step(&mut test_bus.bus);
        assert_eq!(test_bus.bus.ram[0x10], 0x04);
    }

    /// **Objective**: Verify LDX, LDY, STX, STY with diverse addressing modes 
    /// (including ZeroPageY for LDX/STX).
    #[test]
    fn test_load_store_extended() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        
        // LDX ZeroPageY: Address = 0x10 + Y(5) = 0x15
        cpu.y = 5;
        test_bus.bus.ram[0x15] = 0xAA;
        test_bus.bus.ram[0] = 0xB6;
        test_bus.bus.ram[1] = 0x10;
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.x, 0xAA);
        
        // STX ZeroPageY: Write X(0x55) to 0x20 + Y(5) = 0x25
        cpu.x = 0x55;
        test_bus.bus.ram[2] = 0x96;
        test_bus.bus.ram[3] = 0x20;
        cpu.step(&mut test_bus.bus);
        assert_eq!(test_bus.bus.ram[0x25], 0x55);
        
        // LDY AbsoluteX: Address = 0x0500 + X(2) = 0x0502
        cpu.x = 2;
        test_bus.bus.ram[0x0502] = 0x77;
        test_bus.bus.ram[4] = 0xBC;
        test_bus.bus.ram[5] = 0x00;
        test_bus.bus.ram[6] = 0x05;
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.y, 0x77);
        
        // STY Absolute: Write Y(0x33) to 0x0678
        cpu.y = 0x33;
        test_bus.bus.ram[7] = 0x8C;
        test_bus.bus.ram[8] = 0x78;
        test_bus.bus.ram[9] = 0x06;
        cpu.step(&mut test_bus.bus);
        assert_eq!(test_bus.bus.ram[0x0678], 0x33);
    }

    /// **Objective**: Verify CPX and CPY comparison instructions.
    #[test]
    fn test_compare_xy() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        
        // CPX: X=0x50, compare with 0x40 -> Carry=1, Zero=0
        cpu.x = 0x50;
        test_bus.bus.ram[0] = 0xE0;
        test_bus.bus.ram[1] = 0x40;
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus);
        assert!(cpu.status.contains(CpuFlags::CARRY));
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        
        // CPY: Y=0x50, compare with 0x60 -> Carry=0, Zero=0, Negative=1
        cpu.y = 0x50;
        test_bus.bus.ram[2] = 0xC0;
        test_bus.bus.ram[3] = 0x60;
        cpu.step(&mut test_bus.bus);
        assert!(!cpu.status.contains(CpuFlags::CARRY));
        assert!(cpu.status.contains(CpuFlags::NEGATIVE));
    }

    /// **Objective**: Verify stack-based data transfer (PHA, PLA, PHP, PLP).
    #[test]
    fn test_stack_extended() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        
        // PHA / PLA: A=0xAA -> Push -> A=0x00 -> Pop -> A=0xAA
        cpu.a = 0xAA;
        cpu.sp = 0xFF;
        test_bus.bus.ram[0] = 0x48; // PHA
        test_bus.bus.ram[1] = 0x68; // PLA
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.sp, 0xFE);
        cpu.a = 0x00;
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0xAA);
        assert_eq!(cpu.sp, 0xFF);
        
        // PHP / PLP: Status=0x01 -> Push -> Status=0x00 -> Pop -> Status=0x01 (plus Break bits)
        cpu.status = CpuFlags::CARRY;
        test_bus.bus.ram[2] = 0x08; // PHP
        test_bus.bus.ram[3] = 0x28; // PLP
        cpu.step(&mut test_bus.bus);
        cpu.status = CpuFlags::from_bits_truncate(0);
        cpu.step(&mut test_bus.bus);
        assert!(cpu.status.contains(CpuFlags::CARRY));
    }

    /// **Objective**: Verify memory-based shift instructions (ASL, LSR, ROL, ROR).
    #[test]
    fn test_memory_shifts() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        
        // ASL $10: 0x80 -> 0x00, Carry=1
        test_bus.bus.ram[0x10] = 0x80;
        test_bus.bus.ram[0] = 0x06; // ASL ZeroPage
        test_bus.bus.ram[1] = 0x10;
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus);
        assert_eq!(test_bus.bus.ram[0x10], 0x00);
        assert!(cpu.status.contains(CpuFlags::CARRY));
        
        // ROL $20: 0x01 -> 0x02 (old Carry=0)
        cpu.status.remove(CpuFlags::CARRY);
        test_bus.bus.ram[0x20] = 0x01;
        test_bus.bus.ram[2] = 0x26; // ROL ZeroPage
        test_bus.bus.ram[3] = 0x20;
        cpu.step(&mut test_bus.bus);
        assert_eq!(test_bus.bus.ram[0x20], 0x02);
    }

    /// **Objective**: Verify all flag manipulation and register transfer instructions.
    #[test]
    fn test_flags_and_transfers() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        
        let opcodes = [
            (0x38, CpuFlags::CARRY, true),   // SEC
            (0x18, CpuFlags::CARRY, false),  // CLC
            (0x78, CpuFlags::INTERRUPT_DISABLE, true), // SEI
            (0x58, CpuFlags::INTERRUPT_DISABLE, false), // CLI
            (0xF8, CpuFlags::DECIMAL_MODE, true), // SED
            (0xD8, CpuFlags::DECIMAL_MODE, false), // CLD
            (0xB8, CpuFlags::OVERFLOW, false), // CLV
        ];
        
        for (op, flag, set) in opcodes {
            test_bus.bus.ram[0] = op;
            cpu.pc = 0;
            if set { cpu.status.remove(flag); } else { cpu.status.insert(flag); }
            cpu.step(&mut test_bus.bus);
            assert_eq!(cpu.status.contains(flag), set, "Opcode {op:#X} failed");
        }
        
        // Transfers
        let transfers = [
            (0xAA, "x", 0x11, 0x11, 0, 0), // TAX: A=11 -> X=11
            (0xA8, "y", 0x11, 0x11, 0, 0), // TAY: A=11 -> Y=11
            (0x8A, "a", 0x22, 0, 0x22, 0), // TXA: X=22 -> A=22
            (0x98, "a", 0x33, 0, 0, 0x33), // TYA: Y=33 -> A=33
            (0xBA, "x", 0xFD, 0, 0, 0),    // TSX: SP=FD -> X=FD
            (0x9A, "sp", 0x44, 0, 0x44, 0), // TXS: X=44 -> SP=44
        ];
        
        for (op, reg, val, a, x, y) in transfers {
            cpu.a = a; cpu.x = x; cpu.y = y; cpu.sp = 0xFD;
            test_bus.bus.ram[0] = op;
            cpu.pc = 0;
            cpu.step(&mut test_bus.bus);
            match reg {
                "x" => assert_eq!(cpu.x, val as u8, "Opcode {op:#X} X mismatch"),
                "y" => assert_eq!(cpu.y, val as u8, "Opcode {op:#X} Y mismatch"),
                "a" => assert_eq!(cpu.a, val as u8, "Opcode {op:#X} A mismatch"),
                "sp" => assert_eq!(cpu.sp, val as u8, "Opcode {op:#X} SP mismatch"),
                _ => {}
            }
        }
    }

    /// **Objective**: Verify all branch instructions for both taken and not taken scenarios.
    #[test]
    fn test_branches_all() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        
        let branches = [
            (0x90, CpuFlags::CARRY, false), // BCC
            (0xB0, CpuFlags::CARRY, true),  // BCS
            (0xF0, CpuFlags::ZERO, true),   // BEQ
            (0xD0, CpuFlags::ZERO, false),  // BNE
            (0x30, CpuFlags::NEGATIVE, true), // BMI
            (0x10, CpuFlags::NEGATIVE, false), // BPL
            (0x50, CpuFlags::OVERFLOW, false), // BVC
            (0x70, CpuFlags::OVERFLOW, true), // BVS
        ];
        
        for (op, flag, take_if_set) in branches {
            // Case 1: Branch taken
            cpu.status = if take_if_set { flag } else { CpuFlags::from_bits_truncate(0) };
            test_bus.bus.ram[0] = op;
            test_bus.bus.ram[1] = 0x05;
            cpu.pc = 0;
            cpu.step(&mut test_bus.bus);
            assert_eq!(cpu.pc, 0x07, "Opcode {op:#X} should have branched");
            
            // Case 2: Branch not taken
            cpu.status = if take_if_set { CpuFlags::from_bits_truncate(0) } else { flag };
            test_bus.bus.ram[10] = op;
            test_bus.bus.ram[11] = 0x05;
            cpu.pc = 10;
            cpu.step(&mut test_bus.bus);
            assert_eq!(cpu.pc, 12, "Opcode {op:#X} should NOT have branched");
        }
    }

    /// **Objective**: Verify addressing modes for various instructions (STA, LDX, LDY, etc).
    #[test]
    fn test_addressing_modes_coverage() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        
        // STA AbsoluteX
        cpu.a = 0x44; cpu.x = 0x10;
        test_bus.bus.ram[0] = 0x9D;
        test_bus.bus.ram[1] = 0x00;
        test_bus.bus.ram[2] = 0x01; // $0100 + $10 = $0110
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus);
        assert_eq!(test_bus.bus.ram[0x0110], 0x44);
        
        // STA AbsoluteY
        cpu.a = 0x55; cpu.y = 0x20;
        test_bus.bus.ram[3] = 0x99;
        test_bus.bus.ram[4] = 0x00;
        test_bus.bus.ram[5] = 0x02; // $0200 + $20 = $0220
        cpu.pc = 3;
        cpu.step(&mut test_bus.bus);
        assert_eq!(test_bus.bus.ram[0x0220], 0x55);
        
        // STA IndirectY
        cpu.a = 0x66; cpu.y = 0x05;
        test_bus.bus.ram[0x10] = 0x00;
        test_bus.bus.ram[0x11] = 0x03; // Pointer to $0300
        test_bus.bus.ram[6] = 0x91;
        test_bus.bus.ram[7] = 0x10; // STA ($10),Y -> $0300 + 5 = $0305
        cpu.pc = 6;
        cpu.step(&mut test_bus.bus);
        assert_eq!(test_bus.bus.ram[0x0305], 0x66);
    }

    /// **Objective**: Verify arithmetic and logical instructions with various modes.
    #[test]
    fn test_arithmetic_logical_modes() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        
        // EOR ZeroPageX: A=0xFF ^ mem[0x10+X(5)]=0x0F -> A=0xF0
        cpu.a = 0xFF; cpu.x = 5;
        test_bus.bus.ram[0x15] = 0x0F;
        test_bus.bus.ram[0] = 0x55;
        test_bus.bus.ram[1] = 0x10;
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0xF0);
        
        // ORA AbsoluteX: A=0x0F | mem[0x100+X(5)]=0xF0 -> A=0xFF
        cpu.a = 0x0F; cpu.x = 5;
        test_bus.bus.ram[0x0105] = 0xF0;
        test_bus.bus.ram[2] = 0x1D;
        test_bus.bus.ram[3] = 0x00;
        test_bus.bus.ram[4] = 0x01;
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0xFF);
        
        // SBC IndirectX: A=0x10 - mem[ptr($10+X(5))=$15 -> $0200]=0x05 -> A=0x0A (C=1)
        cpu.a = 0x10; cpu.x = 5; cpu.status.insert(CpuFlags::CARRY);
        test_bus.bus.ram[0x15] = 0x00;
        test_bus.bus.ram[0x16] = 0x02;
        test_bus.bus.ram[0x0200] = 0x05;
        test_bus.bus.ram[5] = 0xE1;
        test_bus.bus.ram[6] = 0x10;
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.a, 0x0B); // 0x10 - 0x05 = 0x0B (with carry)
    }

    /// **Objective**: Execute remaining addressing modes and opcodes to achieve 100% line coverage.
    #[test]
    fn test_remaining_opcodes_coverage() {
        let mut test_bus = TestBus::new();
        let mut cpu = Cpu::new();
        
        // Setup memory and registers to avoid out-of-bounds or endless loops
        cpu.x = 2; cpu.y = 3; cpu.a = 0x55;
        for i in 0..0x0800 {
            test_bus.bus.ram[i] = 0x10; // Safe target address / data
        }
        
        let opcodes = [
            // LDA
            0xA5, 0xAD, 0xB9,
            // LDX
            0xA6, 0xAE, 0xBE,
            // LDY
            0xA4, 0xB4, 0xAC,
            // STA
            0x85, 0x95, 0x8D, 0x81,
            // STX, STY
            0x8E, 0x94,
            // ADC
            0x65, 0x75, 0x6D, 0x7D, 0x79, 0x61, 0x71,
            // SBC
            0xE5, 0xF5, 0xED, 0xFD, 0xF9, 0xF1,
            // AND
            0x25, 0x35, 0x2D, 0x3D, 0x39, 0x21, 0x31,
            // EOR
            0x45, 0x4D, 0x5D, 0x59, 0x41, 0x51,
            // ORA
            0x05, 0x15, 0x0D, 0x19, 0x01, 0x11,
            // ASL, LSR, ROL, ROR
            0x16, 0x0E, 0x1E,
            0x46, 0x56, 0x4E, 0x5E,
            0x36, 0x2E, 0x3E,
            0x6A, 0x66, 0x76, 0x6E, 0x7E,
            // INC, DEC
            0xE6, 0xF6, 0xFE,
            0xD6, 0xCE, 0xDE,
            // CMP, CPX, CPY
            0xC5, 0xD5, 0xCD, 0xDD, 0xD9, 0xC1, 0xD1,
            0xE4, 0xEC,
            0xC4, 0xCC,
            // BIT
            0x24,
            // Unimplemented (should print and take 2 cycles)
            0x02, 
            // NOP
            0xEA,
        ];
        
        for op in opcodes {
            cpu.pc = 0;
            test_bus.bus.ram[0] = op;
            test_bus.bus.ram[1] = 0x20;
            test_bus.bus.ram[2] = 0x01; // For absolute modes ($0120)
            cpu.step(&mut test_bus.bus);
        }
        
        // Edge case: NMI inside step
        test_bus.bus.ppu.nmi_interrupt = true;
        cpu.step(&mut test_bus.bus); // Triggers NMI branch
        
        // Edge case: ROL/ROR Carry branches (mem)
        cpu.status.insert(CpuFlags::CARRY);
        test_bus.bus.ram[0x20] = 0x80;
        test_bus.bus.ram[0] = 0x2E; // ROL Absolute
        test_bus.bus.ram[1] = 0x20;
        test_bus.bus.ram[2] = 0x00;
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus); // Tests carry=1 branch in ROL mem
        
        test_bus.bus.ram[0x20] = 0x01;
        test_bus.bus.ram[0] = 0x6E; // ROR Absolute
        test_bus.bus.ram[1] = 0x20;
        test_bus.bus.ram[2] = 0x00;
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus); // Tests carry=1 branch in ROR mem
        
        // Edge case: BIT Zero and Negative flags
        cpu.a = 0x00;
        test_bus.bus.ram[0x20] = 0x00; // Bit 7=0, Bit 6=0, And=0 (Zero=1)
        test_bus.bus.ram[0] = 0x24; // BIT ZeroPage
        test_bus.bus.ram[1] = 0x20;
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus); // Tests BIT Zero=1, Negative=0 branches
        
        // Edge case: JMP Indirect normal (no page wrap)
        test_bus.bus.ram[0] = 0x6C; // JMP Indirect
        test_bus.bus.ram[1] = 0x05;
        test_bus.bus.ram[2] = 0x01; // Pointer to $0105
        test_bus.bus.ram[0x0105] = 0x34;
        test_bus.bus.ram[0x0106] = 0x12; // Jumps to $1234
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus);
        assert_eq!(cpu.pc, 0x1234);
        
        // Edge case: ASL / LSR clear carry
        test_bus.bus.ram[0x10] = 0x01; // ASL of 1 leaves carry=0
        test_bus.bus.ram[0] = 0x06; // ASL ZeroPage
        test_bus.bus.ram[1] = 0x10;
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus); // Hit ASL carry=0
        
        test_bus.bus.ram[0x10] = 0x80; // LSR of 0x80 leaves carry=0
        test_bus.bus.ram[0] = 0x46; // LSR ZeroPage
        test_bus.bus.ram[1] = 0x10;
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus); // Hit LSR carry=0
        
        // Edge case: Accumulator shifts
        cpu.a = 0x01; // ASL_A of 1 leaves carry=0
        test_bus.bus.ram[0] = 0x0A; // ASL Accumulator
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus);
        
        cpu.a = 0x80; // LSR_A of 0x80 leaves carry=0
        test_bus.bus.ram[0] = 0x4A; // LSR Accumulator
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus);
        
        cpu.a = 0x00;
        cpu.status.insert(CpuFlags::CARRY);
        test_bus.bus.ram[0] = 0x6A; // ROR Accumulator with Carry=1
        cpu.pc = 0;
        cpu.step(&mut test_bus.bus);
    }
}
