-- types-nvc*.{vcd,fst}'s design: composite types as nvc dumps them (types.sh makes them).
-- clk rises at 5, 15, 25ns; from 10ns every signal holds the values below, 0 / idle before.
library ieee;
use ieee.std_logic_1164.all;

entity types is
end entity;

architecture sim of types is
    type state_t is (idle, busy, done);
    type pair_t is record
        hi : std_logic_vector(3 downto 0);
        lo : std_logic_vector(3 downto 0);
    end record;
    type pkt_t is record
        valid : std_logic;
        state : state_t;
        pair  : pair_t;
    end record;
    type pkts_t is array (0 to 1) of pkt_t;
    type grid_t is array (0 to 1, 0 to 2) of std_logic_vector(3 downto 0);
    type row_t is array (0 to 2) of std_logic_vector(3 downto 0);
    type rows_t is array (0 to 1) of row_t;
    type link_t is record
        addr : std_logic_vector(7 downto 0);
        data : row_t;
    end record;

    constant PAIR0 : pair_t := (others => (others => '0'));
    constant PKT0  : pkt_t := ('0', idle, PAIR0);

    signal clk  : std_logic := '0';
    signal pkt  : pkt_t := PKT0;                                -- 10ns: '1', busy, (x"A", x"5")
    signal pkts : pkts_t := (others => PKT0);                   -- 10ns: pkts(1).pair.lo x"7"
    signal grid : grid_t := (others => (others => x"0"));      -- 10ns: grid(i, j) = 3 * i + j
    signal rows : rows_t := (others => (others => x"0"));      -- 10ns: rows(i)(j) = 3 * i + j
    signal link : link_t := (x"00", (others => x"0"));          -- 10ns: addr x"3C", data (1, 2, 3)
begin
    clk <= not clk after 5 ns when now < 30 ns;
    pkt <= ('1', busy, (x"A", x"5")) after 10 ns;
    pkts <= (PKT0, ('0', idle, (x"0", x"7"))) after 10 ns;
    grid <= ((x"0", x"1", x"2"), (x"3", x"4", x"5")) after 10 ns;
    rows <= ((x"0", x"1", x"2"), (x"3", x"4", x"5")) after 10 ns;
    link <= (x"3C", (x"1", x"2", x"3")) after 10 ns;

    gen : for i in 0 to 1 generate
        signal p : pair_t := PAIR0; -- 10ns: gen(1).p is (x"9", x"6"), gen(0).p stays 0
    begin
        p <= (x"9", x"6") after 10 ns when i = 1 else PAIR0;
    end generate;
end architecture;
