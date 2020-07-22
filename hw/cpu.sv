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

logic [6:0]  w_decode_op;
logic [4:0]  w_decode_rd;
logic [4:0]  w_decode_rs1;
logic [4:0]  w_decode_rs2;
logic [9:0]  w_decode_func;
logic [31:0] w_decode_imm;
logic        w_decode_valid;

cpu_decode cpu_decode_inst
(
    .i_inst(r_inst_buf),

    .o_op(w_decode_op),
    .o_rd(w_decode_rd),
    .o_rs1(w_decode_rs1),
    .o_rs2(w_decode_rs2),
    .o_func(w_decode_func),
    .o_imm(w_decode_imm),
    .o_valid(w_decode_valid)
);

assign o_is_halted = (r_state == cpu_state_halt);

always_ff @ (posedge i_clk)
    if (i_rst)
        begin
            r_state <= cpu_state_init;
            r_pc <= 0;
            r_inst_buf <= 0;

            o_mem_write_en <= 0;
            o_mem_addr <= 0;
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

                        r_state <= cpu_state_execute;
                    end
                cpu_state_execute:
                    begin
                        // Execute the instruction if it's valid
                        if (w_decode_valid)
                            begin
                                // TODO: Execute instruction here

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
