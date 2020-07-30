`include "common.sv"

module top
(
    input  logic i_clk,
    input  logic i_rst_n,

    input  logic i_data_valid,
    input  logic [7:0] i_data,
    output logic o_input_full,

    input  logic i_data_read,
    output logic [7:0] o_data,
    output logic o_output_empty
);

wire       w_in_fifo_empty;
reg        r_in_fifo_read;
wire [7:0] w_in_fifo_output;

fifo #(.BITS_PER_ELEMENT(8), .MAX_ELEMENTS(16)) input_fifo
(
    .i_clk(i_clk),
    .i_rst(!i_rst_n),

    .o_full(o_input_full),
    .o_empty(w_in_fifo_empty),

    .i_data(i_data),
    .i_write(i_data_valid),

    .o_data(w_in_fifo_output),
    .i_read(r_in_fifo_read)
);

wire       w_out_fifo_full;
reg        r_out_fifo_write;
reg [7:0]  r_out_fifo_input;

fifo #(.BITS_PER_ELEMENT(8), .MAX_ELEMENTS(16)) output_fifo
(
    .i_clk(i_clk),
    .i_rst(!i_rst_n),

    .o_full(w_out_fifo_full),
    .o_empty(o_output_empty),

    .i_data(r_out_fifo_input),
    .i_write(r_out_fifo_write),

    .o_data(o_data),
    .i_read(i_data_read)
);

logic w_is_data_available;
assign w_is_data_available = (!w_in_fifo_empty);

logic w_is_space_available;
assign w_is_space_available = (!w_out_fifo_full);

typedef enum
{
    cmd_state_idle,
    cmd_state_reset,
    cmd_state_read,
    cmd_state_write
} cmd_state;

cmd_state r_state;

logic r_dev_rst;
logic w_dev_rst;
assign w_dev_rst = !i_rst_n || r_dev_rst;

// 4KB of 32 bit registers
localparam REG_SPACE_SIZE = 4 * 1024;

// 1MB of memory
localparam MEM_SIZE = 1024 * 1024;

logic [7:0] r_mem[MEM_SIZE-1:0];

logic                w_cpu_mem_write_en;
common::mem_req_size w_cpu_mem_req_size_out;
logic [31:0]         w_cpu_mem_addr_out;
logic [31:0]         w_cpu_mem_data_out;
logic [31:0]         r_cpu_mem_data_in;
logic                r_cpu_start_signal;
logic                w_cpu_is_idle;

cpu cpu
(
    .i_clk(i_clk),
    .i_rst(!i_rst_n),

    .o_mem_write_en(w_cpu_mem_write_en),
    .o_mem_req_size(w_cpu_mem_req_size_out),
    .o_mem_addr(w_cpu_mem_addr_out),
    .o_mem_data(w_cpu_mem_data_out),

    .i_mem_data(r_cpu_mem_data_in),

    .i_start_signal(r_cpu_start_signal),
    .o_is_idle(w_cpu_is_idle)
);

wire w_cmd_parser_data_valid;
assign w_cmd_parser_data_valid = (r_state == cmd_state_idle) ? r_in_fifo_read : 0;

wire [7:0] w_cmd_parser_data;
assign w_cmd_parser_data = (r_state == cmd_state_idle) ? w_in_fifo_output : 0;

wire           w_cmd_parser_cmd_valid;
wire           w_cmd_parser_cmd_valid_next;
reg            r_cmd_parser_clear_cmd;
common::cmd_id w_cmd_parser_cmd_id;
wire [31:0]    w_cmd_parser_cmd_addr;
wire [31:0]    w_cmd_parser_cmd_size;

cmd_parser cmd_parser
(
    .i_clk(i_clk),
    .i_rst(!i_rst_n),

    .i_data_valid(w_cmd_parser_data_valid),
    .i_data(w_cmd_parser_data),

    .i_clear_cmd(r_cmd_parser_clear_cmd),

    .o_cmd_valid(w_cmd_parser_cmd_valid),
    .o_cmd_valid_next(w_cmd_parser_cmd_valid_next),
    .o_cmd_id(w_cmd_parser_cmd_id),
    .o_cmd_addr(w_cmd_parser_cmd_addr),
    .o_cmd_size(w_cmd_parser_cmd_size)
);


reg [31:0]  r_transfer_cur_addr;
wire [31:0] w_transfer_end_addr;
// Make sure to guard against zero sized transfers
// TODO: We could probably move this responsibility to the parser
assign w_transfer_end_addr = (w_cmd_parser_cmd_size > 0) ? (w_cmd_parser_cmd_addr + (w_cmd_parser_cmd_size - 1))
                                                         : w_cmd_parser_cmd_addr;

wire w_cmd_addr_is_reg;
assign w_cmd_addr_is_reg = (w_cmd_parser_cmd_addr >= 'h3ffff000);

wire [9:0] w_cmd_reg_idx;
assign w_cmd_reg_idx = w_cmd_parser_cmd_addr[11:2];

wire [31:0] w_cmd_reg_data;
assign w_cmd_reg_data = w_cmd_parser_cmd_size;

// TODO: Add actual register addresses

wire [7:0] w_cmd_reg_write_data;
assign w_cmd_reg_write_data = w_cmd_parser_cmd_size[7:0];

reg [31:0] r_reg_read_data;
reg [3:0]  r_reg_read_bytes_remaining;

always_ff @ (posedge i_clk)
    if (!i_rst_n)
        begin
            r_state <= cmd_state_idle;
            r_dev_rst <= 0;
            r_in_fifo_read <= 0;
            r_out_fifo_write <= 0;
            r_cpu_start_signal <= 0;
            r_transfer_cur_addr <= 0;
            r_cmd_parser_clear_cmd <= 0;
            r_reg_read_data <= 0;
            r_reg_read_bytes_remaining <= 0;
        end
    else
        begin
            // The cpu start signal should only ever be active for 1 cycle
            if (r_cpu_start_signal)
                begin
                    r_cpu_start_signal <= 0;
                end

            case (r_state)
                cmd_state_idle:
                    begin
                        r_out_fifo_write <= 0;
                        r_cmd_parser_clear_cmd <= 0;

                        if (r_reg_read_bytes_remaining > 0)
                            begin
                                r_reg_read_bytes_remaining <= r_reg_read_bytes_remaining - 1;
                                r_out_fifo_input <= r_reg_read_data[7:0];
                                r_out_fifo_write <= 1;
                                r_reg_read_data <= (r_reg_read_data >> 8);
                            end

                        if (!w_cpu_is_idle)
                            begin
                                if (w_cpu_mem_write_en)
                                    begin
                                        if (w_cpu_mem_addr_out < MEM_SIZE)
                                            begin
                                                case (w_cpu_mem_req_size_out)
                                                    common::mem_req_size_byte:
                                                        begin
                                                            r_mem[w_cpu_mem_addr_out + 0] <= w_cpu_mem_data_out[7:0];
                                                        end
                                                    common::mem_req_size_half:
                                                        begin
                                                            r_mem[w_cpu_mem_addr_out + 0] <= w_cpu_mem_data_out[7:0];
                                                            r_mem[w_cpu_mem_addr_out + 1] <= w_cpu_mem_data_out[15:8];
                                                        end
                                                    common::mem_req_size_word:
                                                        begin
                                                            r_mem[w_cpu_mem_addr_out + 0] <= w_cpu_mem_data_out[7:0];
                                                            r_mem[w_cpu_mem_addr_out + 1] <= w_cpu_mem_data_out[15:8];
                                                            r_mem[w_cpu_mem_addr_out + 2] <= w_cpu_mem_data_out[23:16];
                                                            r_mem[w_cpu_mem_addr_out + 3] <= w_cpu_mem_data_out[31:24];
                                                        end
                                                    default:
                                                        begin
                                                            // Do nothing
                                                        end
                                                endcase
                                            end
                                        else if (w_cpu_mem_addr_out < MEM_SIZE + REG_SPACE_SIZE)
                                            begin
                                                // TODO: Support register writes from the cpu
                                            end
                                        else
                                            begin
                                                // Drop invalid writes
                                            end
                                    end
                                else
                                    begin
                                        if (w_cpu_mem_addr_out < MEM_SIZE)
                                            begin
                                                case (w_cpu_mem_req_size_out)
                                                    common::mem_req_size_byte:
                                                        begin
                                                            r_cpu_mem_data_in <= { 24'b0, r_mem[w_cpu_mem_addr_out + 0] };
                                                        end
                                                    common::mem_req_size_half:
                                                        begin
                                                            r_cpu_mem_data_in <= { 16'b0, r_mem[w_cpu_mem_addr_out + 1], r_mem[w_cpu_mem_addr_out + 0] };
                                                        end
                                                    common::mem_req_size_word:
                                                        begin
                                                            r_cpu_mem_data_in <= { r_mem[w_cpu_mem_addr_out + 3], r_mem[w_cpu_mem_addr_out + 2], r_mem[w_cpu_mem_addr_out + 1], r_mem[w_cpu_mem_addr_out + 0] };
                                                        end
                                                    default:
                                                        begin
                                                            // Do nothing
                                                        end
                                                endcase
                                            end
                                        else if (w_cpu_mem_addr_out < MEM_SIZE + REG_SPACE_SIZE)
                                            begin
                                                // TODO: Support register reads from the cpu
                                            end
                                        else
                                            begin
                                                // Return 0 for invalid reads
                                                r_cpu_mem_data_in <= 0;
                                            end
                                    end
                            end

                        if (w_cmd_parser_cmd_valid)
                            begin
                                case (w_cmd_parser_cmd_id)
                                    common::cmd_id_reset:
                                    begin
                                        r_state <= cmd_state_reset;
                                        r_dev_rst <= 1;
                                    end
                                    common::cmd_id_read:
                                    begin
                                        if (w_cmd_addr_is_reg)
                                            begin
                                                r_state <= cmd_state_idle;
                                                r_cmd_parser_clear_cmd <= 1;

                                                case (w_cmd_reg_idx)
                                                    0:
                                                        begin
                                                            r_reg_read_data <= { 31'b0, !w_cpu_is_idle };
                                                        end
                                                    default:
                                                        begin
                                                            // Return 0 for unknown registers
                                                            r_reg_read_data <= 0;
                                                        end
                                                endcase

                                                r_reg_read_bytes_remaining <= 4;
                                            end
                                        else
                                            begin
                                                r_state <= cmd_state_read;
                                                r_transfer_cur_addr <= w_cmd_parser_cmd_addr;
                                            end
                                    end
                                    common::cmd_id_write:
                                    begin
                                        if (w_cmd_addr_is_reg)
                                            begin
                                                r_state <= cmd_state_idle;
                                                r_cmd_parser_clear_cmd <= 1;

                                                if (w_cpu_is_idle)
                                                    begin
                                                        case (w_cmd_reg_idx)
                                                            0:
                                                                begin
                                                                    r_cpu_start_signal <= w_cmd_reg_data[0];
                                                                end
                                                            default:
                                                                begin
                                                                    // Do nothing for unknown registers
                                                                end
                                                        endcase
                                                    end
                                                else
                                                    begin
                                                        // Register writes aren't supported during cpu execution
                                                    end
                                            end
                                        else
                                            begin
                                                r_state <= cmd_state_write;
                                                r_transfer_cur_addr <= w_cmd_parser_cmd_addr;

                                                r_in_fifo_read <= w_is_data_available;
                                            end
                                    end
                                    default:
                                    begin
                                        r_state <= cmd_state_idle;
                                    end
                                endcase
                            end
                        else if (w_is_data_available && !r_in_fifo_read)
                        begin
                            r_in_fifo_read <= !r_cmd_parser_clear_cmd;
                        end
                        else if (w_is_data_available && r_in_fifo_read)
                            begin
                                r_in_fifo_read <= !w_cmd_parser_cmd_valid_next;
                            end
                    end
                cmd_state_reset:
                    begin
                        r_state <= cmd_state_idle;
                        r_cmd_parser_clear_cmd <= 1;
                        r_dev_rst <= 0;
                    end
                cmd_state_read:
                    begin
                        // If addr < final_addr
                        if (r_transfer_cur_addr <= w_transfer_end_addr)
                            begin
                                if (w_is_space_available)
                                    begin
                                        r_out_fifo_write <= 1;
                                        r_out_fifo_input <= r_mem[r_transfer_cur_addr];

                                        // If this is the final address, our operation is complete
                                        if (r_transfer_cur_addr == w_transfer_end_addr)
                                            begin
                                                r_state <= cmd_state_idle;
                                                r_cmd_parser_clear_cmd <= 1;
                                            end
                                        else
                                            begin
                                                r_transfer_cur_addr <= r_transfer_cur_addr + 1;
                                            end
                                    end
                                else
                                    // Exit the read loop if we run out of space
                                    begin
                                        r_state <= cmd_state_idle;
                                        r_cmd_parser_clear_cmd <= 1;
                                        r_out_fifo_write <= 0;
                                    end
                            end
                        else
                            begin
                                r_state <= cmd_state_idle;
                                r_cmd_parser_clear_cmd <= 1;
                            end
                    end
                cmd_state_write:
                    begin
                        // If addr < final_addr
                        if (r_transfer_cur_addr <= w_transfer_end_addr)
                            begin
                                if (w_is_data_available)
                                    begin
                                        r_in_fifo_read <= 1;
                                        r_mem[r_transfer_cur_addr] <= w_in_fifo_output;

                                        // If this is the final address, our operation is complete
                                        if (r_transfer_cur_addr == w_transfer_end_addr)
                                            begin
                                                r_state <= cmd_state_idle;
                                                r_cmd_parser_clear_cmd <= 1;
                                                r_in_fifo_read <= 0;
                                            end
                                        else
                                            begin
                                                r_transfer_cur_addr <= r_transfer_cur_addr + 1;
                                            end
                                    end
                                else
                                    // Exit the write loop if we run out of data
                                    begin
                                        r_state <= cmd_state_idle;
                                        r_cmd_parser_clear_cmd <= 1;
                                        r_in_fifo_read <= 0;
                                    end
                            end
                        else
                            begin
                                r_state <= cmd_state_idle;
                                r_cmd_parser_clear_cmd <= 1;
                            end
                    end
                default:
                    begin
                        // TODO: Just return to idle from all other states for now.
                        r_state <= cmd_state_idle;
                        r_cmd_parser_clear_cmd <= 1;
                    end
            endcase
        end

endmodule
