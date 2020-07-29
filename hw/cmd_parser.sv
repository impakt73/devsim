`include "common.sv"

module cmd_parser
(
    input  wire           i_clk,
    input  wire           i_rst,

    input  wire           i_data_valid,
    input  wire [7:0]     i_data,

    input  wire           i_clear_cmd,

    output reg            o_cmd_valid,
    output wire           o_cmd_valid_next,
    output common::cmd_id o_cmd_id,
    output reg [31:0]     o_cmd_addr,
    output reg [31:0]     o_cmd_size
);

reg [63:0] r_cmd_buf;

reg [3:0] r_cmd_buf_byte_counter;

wire [3:0] w_cmd_buf_byte_counter_next;
assign w_cmd_buf_byte_counter_next = r_cmd_buf_byte_counter + 1;

assign o_cmd_valid = r_cmd_buf_byte_counter[3] & !i_clear_cmd;

assign o_cmd_valid_next = w_cmd_buf_byte_counter_next[3] & !i_clear_cmd;

wire [2:0] w_cmd_buf_byte_index;
assign w_cmd_buf_byte_index = o_cmd_valid ? 0 : r_cmd_buf_byte_counter[2:0];

assign o_cmd_id   = r_cmd_buf[63:60];
assign o_cmd_addr = { 2'b0, r_cmd_buf[59:30] };
assign o_cmd_size = { 2'b0, r_cmd_buf[29:0] };

always_ff @ (posedge i_clk)
    if (i_rst)
        begin
            r_cmd_buf <= 0;
            r_cmd_buf_byte_counter <= 0;
        end
    else
        begin
            if (i_clear_cmd)
                begin
                    r_cmd_buf <= 0;
                    r_cmd_buf_byte_counter <= 0;
                end
            else if (!o_cmd_valid && i_data_valid)
                begin
                    r_cmd_buf <= { i_data, 56'b0 } | (r_cmd_buf >> 8);
                    /*
                    case (w_cmd_buf_byte_index)
                        0: r_cmd_buf[7:0]   <= i_data;
                        1: r_cmd_buf[15:8]  <= i_data;
                        2: r_cmd_buf[23:16] <= i_data;
                        3: r_cmd_buf[31:24] <= i_data;
                        4: r_cmd_buf[39:32] <= i_data;
                        5: r_cmd_buf[47:40] <= i_data;
                        6: r_cmd_buf[55:48] <= i_data;
                        7: r_cmd_buf[63:56] <= i_data;
                    endcase
                    */
                    r_cmd_buf_byte_counter <= r_cmd_buf_byte_counter + 1;
                end
        end

endmodule