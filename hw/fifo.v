module fifo
#(
    parameter BITS_PER_ELEMENT = 32,
    parameter MAX_ELEMENTS = 256
)
(
    input                              i_clk,
    input                              i_rst,

    output wire                        o_full,
    output wire                        o_empty,

    input  wire [BITS_PER_ELEMENT-1:0] i_data,
    input  wire                        i_write,

    output reg  [BITS_PER_ELEMENT-1:0] o_data,
    input  wire                        i_read
);

parameter COUNTER_SIZE = $clog2(MAX_ELEMENTS) + 1;

reg [BITS_PER_ELEMENT-1:0] r_data[MAX_ELEMENTS-1:0];
reg [COUNTER_SIZE-1:0]     r_read_ptr;
reg [COUNTER_SIZE-1:0]     r_write_ptr;

assign o_empty = (r_read_ptr == r_write_ptr);
assign o_full  = ((r_read_ptr[COUNTER_SIZE-1] != r_write_ptr[COUNTER_SIZE-1]) &&
                  (r_read_ptr[COUNTER_SIZE-2:0] == r_write_ptr[COUNTER_SIZE-2:0]));

wire [COUNTER_SIZE-1:0] w_next_read_ptr = (r_read_ptr + 1);
wire [COUNTER_SIZE-1:0] w_next_write_ptr = (r_write_ptr + 1);

assign o_data = (o_empty && i_write) ? i_data : r_data[r_read_ptr[COUNTER_SIZE-2:0]];

always @ (posedge i_clk)
    if (i_rst)
        begin
            r_read_ptr  <= 0;
            r_write_ptr <= 0;
        end
    else
        begin
            if (i_read && !o_empty)
                begin
                    r_read_ptr <= w_next_read_ptr;
                end
            if (i_write && !o_full)
                begin
                    r_data[r_write_ptr[COUNTER_SIZE-2:0]] <= i_data;
                    r_write_ptr <= w_next_write_ptr;
                end
        end

endmodule