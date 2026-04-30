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
                // On real NES, BRK pushes PC+2 and flags to stack, then jumps to IRQ vector.
                // It sets the BREAK flag (bit 4) in the pushed status byte.
                7
            }
            0xEA => { // NOP (No Operation)
                2
            }

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
}
