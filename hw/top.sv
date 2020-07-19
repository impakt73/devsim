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

logic [31:0] r_cmd_buf;

logic [2:0] r_cmd_buf_byte_counter;

logic [2:0] r_cmd_buf_byte_counter_next;
assign r_cmd_buf_byte_counter_next = r_cmd_buf_byte_counter + 1;

logic w_is_cmd_valid;
assign w_is_cmd_valid = r_cmd_buf_byte_counter[2];

logic w_is_cmd_valid_next;
assign w_is_cmd_valid_next = r_cmd_buf_byte_counter_next[2];

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

logic [3:0] w_cmd_id;
assign w_cmd_id = r_cmd_buf[31:28];

logic [13:0] w_cmd_addr;
assign w_cmd_addr = r_cmd_buf[27:14];

logic [13:0] w_cmd_size;
assign w_cmd_size = r_cmd_buf[13:0];

logic w_cmd_addr_is_reg;
assign w_cmd_addr_is_reg = r_cmd_buf[27];

logic [7:0] w_cmd_reg_write_data;
assign w_cmd_reg_write_data = r_cmd_buf[7:0];

enum bit[3:0]
{
    cmd_id_reset,
    cmd_id_read,
    cmd_id_write
} cmd_id;

logic [7:0] r_mem[16383:0];

logic r_dev_en;
logic [7:0] r_dev_clk_cnt;
logic [31:0] r_dev_inst_buf;

always_ff @ (posedge i_clk)
    if (!i_rst_n)
        begin
            r_dev_clk_cnt <= 0;
        end
    else if (r_dev_en)
        begin
            r_dev_clk_cnt <= r_dev_clk_cnt + 1;
            if (r_dev_clk_cnt == 255)
                begin
                    r_dev_en <= 0;
                end
            else
                begin
                    r_dev_en <= 1;
                    r_dev_inst_buf <= { r_mem[({ 6'd0, r_dev_clk_cnt } * 4 + 3)], r_mem[({ 6'd0, r_dev_clk_cnt } * 4 + 2)], r_mem[({ 6'd0, r_dev_clk_cnt } * 4 + 1)], r_mem[({ 6'd0, r_dev_clk_cnt } * 4 + 0)] };
                end
        end

always_ff @ (posedge i_clk)
    if (!i_rst_n)
        begin
            r_cmd_buf <= 0;
            r_cmd_buf_byte_counter <= 0;
            r_state <= cmd_state_idle;
            r_dev_rst <= 0;
            r_in_fifo_read <= 0;
            r_out_fifo_write <= 0;
            r_dev_en <= 0;
        end
    else
        begin
            case (r_state)
                cmd_state_idle:
                    begin
                        r_out_fifo_write <= 0;
                        if (w_is_cmd_valid)
                            begin
                                case (w_cmd_id)
                                    cmd_id_reset:
                                    begin
                                        r_state <= cmd_state_reset;
                                        r_dev_rst <= 1;
                                    end
                                    cmd_id_read:
                                    begin
                                        if (w_cmd_addr_is_reg)
                                            begin
                                                r_state <= cmd_state_idle;
                                                r_out_fifo_input <= { 7'd0, r_dev_en };
                                                r_out_fifo_write <= 1;
                                            end
                                        else
                                            begin
                                                r_state <= cmd_state_read;

                                                // Move the end address value into the size field
                                                r_cmd_buf[13:0] <= r_cmd_buf[27:14] + r_cmd_buf[13:0];
                                            end
                                    end
                                    cmd_id_write:
                                    begin
                                        if (w_cmd_addr_is_reg)
                                            begin
                                                r_state <= cmd_state_idle;
                                                r_dev_en <= w_cmd_reg_write_data[0];
                                            end
                                        else
                                            begin
                                                r_state <= cmd_state_write;

                                                // Move the end address value into the size field
                                                r_cmd_buf[13:0] <= r_cmd_buf[27:14] + r_cmd_buf[13:0];

                                                r_in_fifo_read <= w_is_data_available;
                                            end
                                    end
                                    default:
                                    begin
                                        r_state <= cmd_state_idle;
                                    end
                                endcase

                                r_cmd_buf_byte_counter[2] <= 0;
                            end
                        else if (w_is_data_available && !r_in_fifo_read)
                        begin
                            r_in_fifo_read <= 1;
                        end
                        else if (w_is_data_available && r_in_fifo_read)
                            begin
                                //r_cmd_buf <= (r_cmd_buf << 8) | { 24'b0, w_in_fifo_output };
                                case (r_cmd_buf_byte_counter)
                                    0: r_cmd_buf[7:0] <= w_in_fifo_output;
                                    1: r_cmd_buf[15:8] <= w_in_fifo_output;
                                    2: r_cmd_buf[23:16] <= w_in_fifo_output;
                                    3: r_cmd_buf[31:24] <= w_in_fifo_output;
                                endcase
                                r_cmd_buf_byte_counter <= r_cmd_buf_byte_counter_next;
                                r_in_fifo_read <= !w_is_cmd_valid_next;
                            end
                    end
                cmd_state_reset:
                    begin
                        r_state <= cmd_state_idle;
                        r_dev_rst <= 0;
                    end
                cmd_state_read:
                    begin
                        // If addr < final_addr
                        if (r_cmd_buf[27:14] <= r_cmd_buf[13:0])
                            begin
                                if (w_is_space_available)
                                    begin
                                        r_out_fifo_write <= 1;
                                        r_out_fifo_input <= r_mem[r_cmd_buf[27:14]];

                                        // If this is the final address, our operation is complete
                                        if (r_cmd_buf[27:14] == r_cmd_buf[13:0])
                                            begin
                                                r_state <= cmd_state_idle;
                                            end
                                        else
                                            begin
                                                r_cmd_buf[27:14] <= r_cmd_buf[27:14] + 1;
                                            end
                                    end
                                else
                                    // Exit the read loop if we run out of space
                                    begin
                                        r_state <= cmd_state_idle;
                                        r_out_fifo_write <= 0;
                                    end
                            end
                        else
                            begin
                                r_state <= cmd_state_idle;
                            end
                    end
                cmd_state_write:
                    begin
                        // If addr < final_addr
                        if (r_cmd_buf[27:14] <= r_cmd_buf[13:0])
                            begin
                                if (w_is_data_available)
                                    begin
                                        r_in_fifo_read <= 1;
                                        r_mem[r_cmd_buf[27:14]] <= w_in_fifo_output;

                                        // If this is the final address, our operation is complete
                                        if (r_cmd_buf[27:14] == r_cmd_buf[13:0])
                                            begin
                                                r_state <= cmd_state_idle;
                                            end
                                        else
                                            begin
                                                r_cmd_buf[27:14] <= r_cmd_buf[27:14] + 1;
                                            end
                                    end
                                else
                                    // Exit the write loop if we run out of data
                                    begin
                                        r_state <= cmd_state_idle;
                                        r_in_fifo_read <= 0;
                                    end
                            end
                        else
                            begin
                                r_state <= cmd_state_idle;
                            end
                    end
                default:
                    begin
                        // TODO: Just return to idle from all other states for now.
                        r_state <= cmd_state_idle;
                    end
            endcase
        end

endmodule
