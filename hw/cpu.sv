module cpu
(
    input  logic i_clk,
    input  logic i_rst,

    input  logic i_enable,

    output logic        o_mem_write_en,
    output logic [13:0] o_mem_addr,
    output logic [31:0] o_mem_data,

    input  logic [31:0] i_mem_data,

    output logic o_is_halted
);

logic [31:0] r_pc;
logic [31:0] r_inst_buf;

reg [31:0] r_regs[30:0];

typedef enum
{
    cpu_state_init,
    cpu_state_fetch,
    cpu_state_fetch_wait,
    cpu_state_decode,
    cpu_state_execute,
    cpu_state_halt
} cpu_state;

cpu_state r_state;

typedef enum
{
    inst_fmt_type_invalid,
    inst_fmt_type_r,
    inst_fmt_type_i,
    inst_fmt_type_s,
    inst_fmt_type_b,
    inst_fmt_type_u,
    inst_fmt_type_j
} inst_fmt_type;

// TODO: This is a test register
inst_fmt_type r_inst_fmt_type;

wire [4:0] w_inst_rd;
assign w_inst_rd = r_inst_buf[11:7];

wire [4:0] w_inst_rs1;
assign w_inst_rs1 = r_inst_buf[19:15];

wire [4:0] w_inst_rs2;
assign w_inst_rs2 = r_inst_buf[24:20];

wire [9:0] w_inst_func;
assign w_inst_func = { r_inst_buf[31:25], r_inst_buf[14:12] };

wire [11:0] w_inst_i_imm;
assign w_inst_i_imm = { r_inst_buf[31:20] };

wire [11:0] w_inst_s_imm;
assign w_inst_s_imm = { r_inst_buf[31:25], r_inst_buf[11:7] };

wire [11:0] w_inst_b_imm;
assign w_inst_b_imm = { r_inst_buf[31], r_inst_buf[7], r_inst_buf[30:25], r_inst_buf[11:8] };

wire [19:0] w_inst_u_imm;
assign w_inst_u_imm = { r_inst_buf[31:12] };

wire [19:0] w_inst_j_imm;
assign w_inst_j_imm = { r_inst_buf[31], r_inst_buf[19:12], r_inst_buf[20], r_inst_buf[30:21] };

reg [31:0] r_inst_imm;

assign o_is_halted = (r_state == cpu_state_halt);

always_ff @ (posedge i_clk)
    if (i_rst)
        begin
            r_state <= cpu_state_init;
            r_pc <= 0;
            r_inst_buf <= 0;

            o_mem_write_en <= 0;
            o_mem_addr <= 0;

            // TODO: These probably shouldn't be registers?
            r_inst_fmt_type <= inst_fmt_type_invalid;
            r_inst_imm <= 0;
        end
    else if (i_enable)
        begin
            case (r_state)
                cpu_state_init:
                    begin
                        // Only start execution from the halted state if we're at pc 0
                        if (r_pc == 0)
                            begin
                                r_state <= cpu_state_fetch;
                            end
                    end
                cpu_state_fetch:
                    begin
                        o_mem_addr <= r_pc[13:0];

                        r_state <= cpu_state_fetch_wait;
                    end
                cpu_state_fetch_wait:
                    begin
                        r_pc <= r_pc + 4;

                        r_state <= cpu_state_decode;
                    end
                cpu_state_decode:
                    begin
                        r_inst_buf <= i_mem_data;

                        // Determine the instruction format type so we can decode instruction fields
                        case (i_mem_data[6:0])
                            'b0110111,
                            'b0010111: r_inst_fmt_type <= inst_fmt_type_u;
                            'b1101111: r_inst_fmt_type <= inst_fmt_type_j;
                            'b1100011: r_inst_fmt_type <= inst_fmt_type_b;
                            'b0100011: r_inst_fmt_type <= inst_fmt_type_s;
                            'b0110011: r_inst_fmt_type <= inst_fmt_type_r;
                            'b1100111,
                            'b0000011,
                            'b0010011,
                            'b0001111,
                            'b1110011: r_inst_fmt_type <= inst_fmt_type_i;
                            default: r_inst_fmt_type <= inst_fmt_type_invalid;
                        endcase

                        r_state <= cpu_state_execute;
                    end
                cpu_state_execute:
                    begin
                        // Execute the instruction if it's valid
                        if (r_inst_fmt_type != inst_fmt_type_invalid)
                            begin
                                // TODO: This should definitely be calculated during the decode stage
                                case (r_inst_fmt_type)
                                    inst_fmt_type_i: r_inst_imm <= { { 20 { w_inst_i_imm[11] } }, w_inst_i_imm[11:0] };
                                    inst_fmt_type_s: r_inst_imm <= { { 20 { w_inst_s_imm[11] } }, w_inst_s_imm[11:0] };
                                    inst_fmt_type_b: r_inst_imm <= { { 19 { w_inst_b_imm[11] } }, w_inst_b_imm[11:0], 1'b0 };
                                    inst_fmt_type_u: r_inst_imm <= { { 12 { w_inst_u_imm[19] } }, w_inst_u_imm[19:0] };
                                    inst_fmt_type_j: r_inst_imm <= { { 11 { w_inst_j_imm[19] } }, w_inst_j_imm[19:0], 1'b0 };
                                    default:         r_inst_imm <= 0;
                                endcase

                                r_state <= cpu_state_fetch;
                            end
                        else
                            begin
                                // Move to the halted state if we encounter an invalid instruction
                                r_state <= cpu_state_halt;
                            end
                    end
                cpu_state_halt:
                    begin
                        // We stay in this state until reset
                        r_state <= cpu_state_halt;
                    end
            endcase
        end

endmodule
