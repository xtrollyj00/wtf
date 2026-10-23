-- vhdl.fst's design, dumped by nvc as it names VHDL: run `nvc -a vhdl.vhd -e vhdl -r --wave=vhdl.fst
-- --dump-arrays --stop-time=30ns` here. CLK rises at 5, 15, 25ns; ARR(0) is 3 and ARR(1) is 0xC
-- from 10ns; GEN(1).X is high from 10ns.
library ieee;
use ieee.std_logic_1164.all;

entity vhdl is
end entity;

architecture sim of vhdl is
    type arr_t is array (0 to 1) of std_logic_vector(3 downto 0);
    signal CLK : std_logic := '0';
    signal ARR : arr_t := (others => (others => '0'));
begin
    CLK <= not CLK after 5 ns when now < 30 ns;
    ARR <= (x"3", x"C") after 10 ns;

    GEN : for i in 0 to 1 generate
        signal X : std_logic := '0';
    begin
        X <= '1' after 10 ns when i = 1 else '0';
    end generate;
end architecture;
