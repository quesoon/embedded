use embassy_stm32::gpio::{Flex, Level, Output, Pin, Pull, Speed};
use embassy_stm32::{into_ref, Peripheral};

mod instructions;

/*
 TODO:
  • Манипуляция с дисплеем                                                [+]
  • Манипуляция с яркостью                                                [+]
  • Манипуляции с сегментами                                              [+]
  • Манипуляции со светодиодами                                           [+]
  • Манипуляции с кнопками                                                [+]
  • Проверка значений на этапе компиляции                                 [-]
  • def_pressed_keys - разобраться (как вернуть массив из функции?)       [+]
 */

pub struct LedAndKey<'d, STB: Pin, CLK: Pin, DIO: Pin> {
    stb: Output<'d, STB>,
    clk: Output<'d, CLK>,
    dio: Flex<'d, DIO>,
    display: bool,
    brightness: u8,
}

impl<'d, STB: Pin, CLK: Pin, DIO: Pin> LedAndKey<'d, STB, CLK, DIO> {
    pub(crate) fn new(stb: impl Peripheral<P=STB> + 'static,
                      clk: impl Peripheral<P=CLK> + 'static,
                      dio: impl Peripheral<P=DIO> + 'static) -> LedAndKey<'d, STB, CLK, DIO> {
        into_ref!(stb, clk, dio);

        let mut clk: Output<CLK> = Output::new(clk, Level::Low, Speed::Low);
        let mut dio: Flex<DIO> = Flex::new(dio);
        let mut stb: Output<STB> = Output::new(stb, Level::Low, Speed::Low);
        let mut display: bool = true;
        let mut brightness: u8 = instructions::BRIGHTNESS;

        stb.set_high();
        dio.set_low();
        clk.set_low();
        dio.set_as_output(Speed::Low); // By default, in data transfer mode.

        let mut driver = Self { stb, dio, clk, display, brightness };
        driver.push_display_ctrl_instr();
        driver.cleanup();

        driver
    }

    // Includes display.
    pub(crate) fn display_on(&mut self) -> () {
        self.display = true;
        self.push_display_ctrl_instr();
    }

    // Disable display.
    pub(crate) fn display_off(&mut self) -> () {
        self.display = false;
        self.push_display_ctrl_instr();
    }

    // Sets all display registers to zero.
    pub(crate) fn cleanup(&mut self) -> () {
        self.push_data_write_instr();
        self.stb.set_low();
        self.push_address_instr(instructions::NULL);

        for i in 0..15 {
            self.write_byte(instructions::NULL);
        }

        self.stb.set_low();
    }

    /*
     Sets the brightness of the LEDs and segments.
     @value: 0..7
     */
    pub(crate) fn set_brightness(&mut self, value: u8) -> () {
        self.brightness = value;
        self.push_display_ctrl_instr();
    }

    /*
     Sets the value of the segment.
     @position: 0..7
     @state: 0..9 and A-Z
     */
    pub(crate) fn set_segment_value(&mut self, position: u8, value: u8) -> () {
        self.write(position << 1, value);
    }

    /*
     Sets the LED's value.
     @position: 0..7
     @state: 0 or 1
     */
    pub(crate) fn set_led_state(&mut self, position: u8, state: u8) -> () {
        self.write((position << 1) + 1, state);
    }

    /*
     Determines the key pressed.
     Returns an array of states for each key, from left to right: true - pressed, false - otherwise.
    */
    pub(crate) fn def_pressed_keys<'a>(&'a mut self, keys_array: &'a mut [bool; 8]) -> &mut [bool; 8] {
        let mut data: u32 = self.scan_keys();

        for i in 0..4 {
            keys_array[i] = if (data >> (8 * i) & 1) == 1 { true } else { false };
            keys_array[i + 4] = if (data >> (8 * i + 4) & 1) == 1 { true } else { false };
        }

        keys_array
    }

    /*
     Write a byte to the display register.
     @position: 0..15
     */
    fn write(&mut self, position: u8, data: u8) -> () {
        self.push_data_write_instr();

        self.stb.set_low();
        self.push_address_instr(position);
        self.write_byte(data);
        self.stb.set_high();
    }

    // Reads the values of each button.
    pub(crate) fn scan_keys(&mut self) -> u32 {
        self.stb.set_low();
        self.write_byte(instructions::SET_DATA_INSTR | instructions::DATA_READ_INSTR);

        let mut data: u32 = 0;
        for i in 0..4 { data |= (self.read_byte() as u32) << (i * 8); }

        self.stb.set_high();

        data
    }

    /*
     Display configuration instruction.
     Default:
     ~ display on
     ~ brightness max (0x07)
     */
    fn push_display_ctrl_instr(&mut self) -> () {
        self.stb.set_high();
        self.dio.set_low();
        self.clk.set_low();

        let display_instr: u8;

        if self.display {
            display_instr = instructions::DISPLAY_ON_INSTR;
        } else {
            display_instr = instructions::DISPLAY_OFF_INSTR;
        }

        self.push_instruction(instructions::SET_DISPLAY_CTRL_INSTR |
            display_instr | self.brightness);
    }

    /*
     Sends instructions for subsequent recording.
     Data command: AUTOMATIC address increment, normal mode.
     */
    fn push_data_write_instr(&mut self) -> () {
        self.push_instruction(instructions::SET_DATA_INSTR |
            instructions::DATA_WRITE_INSTR);
    }

    // Sets the address to write the value to.
    fn push_address_instr(&mut self, address: u8) -> () {
        self.write_byte(instructions::SET_ADDRESS_INSTR | address);
    }

    // Push a instruction to the TM1638.
    fn push_instruction(&mut self, instruction: u8) -> () {
        self.stb.set_low();
        self.write_byte(instruction);
        self.stb.set_high();
    }

    // Write 1 byte of information to the TM1638.
    fn write_byte(&mut self, byte: u8) -> () {
        for i in 0..8 {
            self.clk.set_low();

            if (byte >> i) & 1 == 0 { self.dio.set_low(); } else { self.dio.set_high(); }

            self.clk.set_high();
        }
    }

    // Read 1 byte of information from TM1638.
    fn read_byte(&mut self) -> u8 {
        self.dio.set_as_input(Pull::Up);

        let mut byte: u8 = 0;
        for i in 0..8 {
            self.clk.set_low();
            self.clk.set_high();

            if self.dio.is_high() { byte |= 1 << i; }
        }

        self.dio.set_as_output(Speed::Low);

        byte
    }
}
