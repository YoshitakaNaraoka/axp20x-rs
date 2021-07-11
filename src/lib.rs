#![no_std]

use embedded_hal::blocking::{
    delay::DelayMs,
    i2c::{Write, WriteRead},
};

use core::ops::{BitAnd, BitOr};

use bitmask_enum::bitmask;
use num_enum::{FromPrimitive, IntoPrimitive};

const DEFAULT_AXP202_SLAVE_ADDR: u8 = 0x35;
const BATTERY_VOLTAGE_STEP: f32 = 1.1;

#[derive(Debug)]
pub enum PowerState {
    On,
    Off,
}

#[bitmask(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum PowerInputStatus {
    BootSource = Self(1 << 0),
    AcinVbusShortCircuit = Self(1 << 1),
    CurrentDirection = Self(1 << 2),
    VbusAbove = Self(1 << 3),
    VbusUsable = Self(1 << 4),
    VbusPresence = Self(1 << 5),
    AcinUsable = Self(1 << 6),
    AcinPresence = Self(1 << 7),
}

#[bitmask(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Power {
    Exten = Self(1 << 0),
    DcDc3 = Self(1 << 1),
    Ldo2 = Self(1 << 2),
    Ldo4 = Self(1 << 3),
    DcDc2 = Self(1 << 4),
    Ldo3 = Self(1 << 6),
}

#[bitmask(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Charge {
    Charging = Self(1 << 7),
}

#[bitmask(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventsIrq {
    PowerKeyShortPress = Self(1 << 17),

    Int1 = Self(0xFF),
    Int2 = Self(0xFF00),
    Int3 = Self(0xFF0000),
    Int4 = Self(0xFF000000),
    Int5 = Self(0xFF00000000),
}

impl EventsIrq {
    fn is_int1(&self) -> bool {
        self.intersects(Self::Int1)
    }

    fn is_int2(&self) -> bool {
        self.intersects(Self::Int2)
    }

    fn is_int3(&self) -> bool {
        self.intersects(Self::Int3)
    }

    fn is_int4(&self) -> bool {
        self.intersects(Self::Int4)
    }

    fn is_int5(&self) -> bool {
        self.intersects(Self::Int5)
    }

    fn into_int1_u8(&self) -> u8 {
        let mask: u64 = self.bitand(Self::Int1).into();
        mask as u8
    }

    fn into_int2_u8(&self) -> u8 {
        let mask: u64 = self.bitand(Self::Int2).into();
        (mask >> 8) as u8
    }

    fn into_int3_u8(&self) -> u8 {
        let mask: u64 = self.bitand(Self::Int3).into();
        (mask >> 16) as u8
    }

    fn into_int4_u8(&self) -> u8 {
        let mask: u64 = self.bitand(Self::Int4).into();
        (mask >> 24) as u8
    }

    fn into_int5_u8(&self) -> u8 {
        let mask: u64 = self.bitand(Self::Int5).into();
        (mask >> 32) as u8
    }

    fn from_int1_u8(val: u8) -> Self {
        let mask: u64 = val as u64;
        Self::Int1.bitand(mask.into())
    }

    fn from_int2_u8(val: u8) -> Self {
        let mask: u64 = (val as u64)
            .checked_shl(8)
            .expect("Source being u8, this should not overflow");
        Self::Int2.bitand(mask.into())
    }

    fn from_int3_u8(val: u8) -> Self {
        let mask: u64 = (val as u64)
            .checked_shl(16)
            .expect("Source being u8, this should not overflow");
        Self::Int3.bitand(mask.into())
    }

    fn from_int4_u8(val: u8) -> Self {
        let mask: u64 = (val as u64)
            .checked_shl(24)
            .expect("Source being u8, this should not overflow");
        Self::Int4.bitand(mask.into())
    }

    fn from_int5_u8(val: u8) -> Self {
        let mask: u64 = (val as u64)
            .checked_shl(32)
            .expect("Source being u8, this should not overflow");
        Self::Int5.bitand(mask.into())
    }

    fn toggle(self, current_mask: EventsIrq, enable: bool) -> Self {
        if enable {
            self.bitor(current_mask)
        } else {
            self.bitand(!current_mask)
        }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, IntoPrimitive)]
enum Register {
    PowerInputStatus = 0x00,
    PowerWorkingModeChargeStatus = 0x01,
    IcType = 0x03,
    Ldo234Dc23Ctl = 0x12,
    Charge1 = 0x33,
    EnabledIrq1 = 0x40,
    EnabledIrq2 = 0x41,
    EnabledIrq3 = 0x42,
    EnabledIrq4 = 0x43,
    EnabledIrq5 = 0x45,
    StatusIrq1 = 0x48,
    StatusIrq2 = 0x49,
    StatusIrq3 = 0x4A,
    StatusIrq4 = 0x4B,
    StatusIrq5 = 0x4C,
    BatteryAverageVoltageHigh8b = 0x78,
    BatteryAverageVoltageLow4b = 0x79,
    BatteryPercentage = 0xB9,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, IntoPrimitive, FromPrimitive)]
enum ChipId {
    #[default]
    Unknown = 0x00,
    Axp202 = 0x41,
    Axp192 = 0x03,
    Axp173 = 0xAD,
}

#[derive(Debug)]
pub enum AxpError {
    I2cError,
    Uninitialized,
}

pub struct Axpxx<I2C>
where
    I2C: Write + WriteRead,
{
    i2c: I2C,
    address: u8,
    state: State,
}

enum State {
    Uninitialized,
    Initialized(ChipId),
}

impl<I2C> Axpxx<I2C>
where
    I2C: Write + WriteRead,
{
    pub fn new(i2c: I2C) -> Self {
        Self {
            i2c,
            address: DEFAULT_AXP202_SLAVE_ADDR,
            state: State::Uninitialized,
        }
    }

    pub fn new_with_address(i2c: I2C, address: u8) -> Self {
        Self {
            i2c,
            address,
            state: State::Uninitialized,
        }
    }

    pub fn init(&mut self) -> Result<(), AxpError> {
        let chip_id = self.probe_chip()?;
        self.state = State::Initialized(chip_id);
        Ok(())
    }

    fn read_reg(&mut self, reg: Register) -> Result<u8, AxpError> {
        let mut buf = [0u8; 1];
        let read_buf = [reg.into(); 1];
        self.i2c
            .write_read(self.address, &read_buf, &mut buf)
            .map_err(|_| AxpError::I2cError)
            .and_then(|_| Ok(buf[0]))
    }

    fn write_reg(&mut self, reg: Register, val: u8) -> Result<(), AxpError> {
        self.i2c
            .write(self.address, &[reg.into(), val])
            .map_err(|_| AxpError::I2cError)
    }

    fn probe_chip(&mut self) -> Result<ChipId, AxpError> {
        let chip_id = self.read_reg(Register::IcType)?;
        Ok(ChipId::from(chip_id))
    }

    pub fn is_acin_present(&mut self) -> Result<bool, AxpError> {
        let power_status = self.read_reg(Register::PowerInputStatus)?;
        let power_status = PowerInputStatus(power_status);
        Ok(power_status.intersects(PowerInputStatus::AcinPresence))
    }

    pub fn is_acin_usable(&mut self) -> Result<bool, AxpError> {
        let power_status = self.read_reg(Register::PowerInputStatus)?;
        let power_status = PowerInputStatus(power_status);
        Ok(power_status.intersects(PowerInputStatus::AcinUsable))
    }

    pub fn is_vbus_present(&mut self) -> Result<bool, AxpError> {
        let power_status = self.read_reg(Register::PowerInputStatus)?;
        let power_status = PowerInputStatus(power_status);
        Ok(power_status.intersects(PowerInputStatus::VbusPresence))
    }

    pub fn is_vbus_usable(&mut self) -> Result<bool, AxpError> {
        let power_status = self.read_reg(Register::PowerInputStatus)?;
        let power_status = PowerInputStatus(power_status);
        Ok(power_status.intersects(PowerInputStatus::VbusUsable))
    }

    pub fn is_vbus_above(&mut self) -> Result<bool, AxpError> {
        let power_status = self.read_reg(Register::PowerInputStatus)?;
        let power_status = PowerInputStatus(power_status);
        Ok(power_status.intersects(PowerInputStatus::VbusAbove))
    }

    pub fn is_battery_charging(&mut self) -> Result<bool, AxpError> {
        let raw_charge1 = self.read_reg(Register::Charge1)?;
        Ok(Charge(raw_charge1).intersects(Charge::Charging))
    }

    pub fn is_acin_vbus_shortcircuit(&mut self) -> Result<bool, AxpError> {
        let power_status = self.read_reg(Register::PowerInputStatus)?;
        let power_status = PowerInputStatus(power_status);
        Ok(power_status.intersects(PowerInputStatus::AcinVbusShortCircuit))
    }

    pub fn is_bootsource_acin_vbus(&mut self) -> Result<bool, AxpError> {
        let power_status = self.read_reg(Register::PowerInputStatus)?;
        let power_status = PowerInputStatus(power_status);
        Ok(power_status.intersects(PowerInputStatus::BootSource))
    }

    pub fn get_battery_percentage(&mut self) -> Result<u8, AxpError> {
        self.read_reg(Register::BatteryPercentage)
    }

    pub fn get_battery_voltage(&mut self) -> Result<f32, AxpError> {
        let battery_high_8b = self.read_reg(Register::BatteryAverageVoltageHigh8b)?;
        let battery_low_4b = self.read_reg(Register::BatteryAverageVoltageLow4b)?;
        Ok(
            (((battery_high_8b as u16) << 4) | (battery_low_4b & 0x0F) as u16) as f32
                * BATTERY_VOLTAGE_STEP,
        )
    }

    pub fn toggle_irq(&mut self, irqs: EventsIrq, enable: bool) -> Result<(), AxpError> {
        if irqs.is_int1() {
            let irq1 = self.read_reg(Register::EnabledIrq1)?;
            let irq1 = EventsIrq::from_int1_u8(irq1);
            let irqs = irqs.toggle(irq1, enable);
            self.write_reg(Register::EnabledIrq1, irqs.into_int1_u8())?;
        }
        if irqs.is_int2() {
            let irq2 = self.read_reg(Register::EnabledIrq2)?;
            let irq2 = EventsIrq::from_int2_u8(irq2).bitor(irqs);
            let irqs = irqs.toggle(irq2, enable);
            self.write_reg(Register::EnabledIrq2, irqs.into_int2_u8())?;
        }
        if irqs.is_int3() {
            let irq3 = self.read_reg(Register::EnabledIrq3)?;
            let irq3 = EventsIrq::from_int3_u8(irq3).bitor(irqs);
            let irqs = irqs.toggle(irq3, enable);
            self.write_reg(Register::EnabledIrq3, irqs.into_int3_u8())?;
        }
        if irqs.is_int4() {
            let irq4 = self.read_reg(Register::EnabledIrq4)?;
            let irq4 = EventsIrq::from_int4_u8(irq4).bitor(irqs);
            let irqs = irqs.toggle(irq4, enable);
            self.write_reg(Register::EnabledIrq4, irqs.into_int4_u8())?;
        }
        if irqs.is_int5() {
            let irq5 = self.read_reg(Register::EnabledIrq5)?;
            let irq5 = EventsIrq::from_int5_u8(irq5).bitor(irqs);
            let irqs = irqs.toggle(irq5, enable);
            self.write_reg(Register::EnabledIrq5, irqs.into_int5_u8())?;
        }
        Ok(())
    }

    pub fn clear_irq(&mut self) -> Result<(), AxpError> {
        self.write_reg(Register::StatusIrq1, 0xFF)?;
        self.write_reg(Register::StatusIrq2, 0xFF)?;
        self.write_reg(Register::StatusIrq3, 0xFF)?;
        self.write_reg(Register::StatusIrq4, 0xFF)?;
        self.write_reg(Register::StatusIrq5, 0xFF)?;
        Ok(())
    }

    pub fn read_irq(&mut self) -> Result<EventsIrq, AxpError> {
        let irq1 = self.read_reg(Register::StatusIrq1)?;
        let irq2 = self.read_reg(Register::StatusIrq2)?;
        let irq3 = self.read_reg(Register::StatusIrq3)?;
        let irq4 = self.read_reg(Register::StatusIrq4)?;
        let irq5 = self.read_reg(Register::StatusIrq5)?;
        self.clear_irq()?;
        Ok(EventsIrq::from_int1_u8(irq1)
            .bitor(EventsIrq::from_int2_u8(irq2))
            .bitor(EventsIrq::from_int3_u8(irq3))
            .bitor(EventsIrq::from_int4_u8(irq4))
            .bitor(EventsIrq::from_int5_u8(irq5)))
    }

    pub fn set_power_output(
        &mut self,
        channel: Power,
        state: PowerState,
        delay: &mut impl DelayMs<u32>,
    ) -> Result<(), AxpError> {
        match self.state {
            State::Uninitialized => Err(AxpError::Uninitialized),
            State::Initialized(chip_id) => {
                // Before setting, the output cannot be all turned off
                let mut data: u8;
                loop {
                    data = self.read_reg(Register::Ldo234Dc23Ctl)?;
                    delay.delay_ms(10);
                    if data != 0 {
                        break;
                    }
                }

                let mut data = Power::from(data);

                match state {
                    PowerState::On => {
                        data |= channel;
                    }
                    PowerState::Off => {
                        data &= !channel;
                    }
                };

                if chip_id == ChipId::Axp202 {
                    data |= Power::DcDc3.into();
                }
                self.write_reg(Register::Ldo234Dc23Ctl, u8::from(data))
            }
        }
    }
}
