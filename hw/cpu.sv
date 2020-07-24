`include "common.sv"

module cpu
(
    input  logic                i_clk,
    input  logic                i_rst,

    input  logic                i_enable,

    output logic                o_mem_write_en,
    output common::mem_req_size o_mem_req_size,
    output logic [31:0]         o_mem_addr,
    output logic [31:0]         o_mem_data,

    input  logic [31:0]         i_mem_data,

    output logic                o_is_halted
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
    cpu_state_memory_load_wait,
    cpu_state_memory_load_execute,
    cpu_state_halt
} cpu_state;

cpu_state r_state;

logic [6:0]  w_decode_op;
logic [4:0]  w_decode_rd;
logic [4:0]  w_decode_rs1;
logic [4:0]  w_decode_rs2;
logic [9:0]  w_decode_func;
logic [19:0] w_decode_imm;
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

logic w_decode_rd_is_valid;
assign w_decode_rd_is_valid = (w_decode_rd != 0);

logic [4:0] w_decode_rd_idx;
assign w_decode_rd_idx = (w_decode_rd - 1);

logic [4:0] w_decode_rs1_idx;
assign w_decode_rs1_idx = (w_decode_rs1 - 1);

logic [4:0] w_decode_rs2_idx;
assign w_decode_rs2_idx = (w_decode_rs2 - 1);

assign o_is_halted = (r_state == cpu_state_halt);

always_ff @ (posedge i_clk)
    if (i_rst)
        begin
            r_state <= cpu_state_init;
            r_pc <= 0;
            r_inst_buf <= 0;

            o_mem_write_en <= 0;
            o_mem_req_size <= common::mem_req_size_word;
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
                        o_mem_write_en <= 0;
                        o_mem_req_size <= common::mem_req_size_word;
                        o_mem_addr <= r_pc;

                        r_state <= cpu_state_fetch_wait;
                    end
                cpu_state_fetch_wait:
                    begin
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
                                // Default to moving the PC to the next instruction
                                // Several instructions may override this behavior
                                r_pc <= r_pc + 4;

                                // Default to moving back to the fetch state
                                // Memory instructions will override this and move to their own special stage since they take
                                // longer to execute than normal instructions.
                                r_state <= cpu_state_fetch;

                                casez({ w_decode_func, w_decode_op })

                                    // lui
                                    17'b??????????0110111:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= { w_decode_imm, 12'b0 };
                                                end
                                        end

                                    // auipc
                                    17'b??????????0010111:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_pc + { w_decode_imm, 12'b0 };
                                                end
                                        end

                                    // jal
                                    17'b??????????1101111:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_pc + 4;
                                                end

                                            r_pc <= r_pc + { { 11 { w_decode_imm[19] } }, w_decode_imm, 1'b0 };
                                        end

                                    // jalr
                                    17'b???????0001100111:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_pc + 4;
                                                end

                                            r_pc <= (r_regs[w_decode_rs1_idx] + { { 12 { w_decode_imm[19] } }, w_decode_imm }) & 'hfffffffe;
                                        end

                                    // beq
                                    17'b???????0001100011:
                                        begin
                                            if (r_regs[w_decode_rs1_idx] == r_regs[w_decode_rs2_idx])
                                                begin
                                                    r_pc <= r_pc + { { 11 { w_decode_imm[19] } }, w_decode_imm, 1'b0 };
                                                end
                                        end

                                    // bne
                                    17'b???????0011100011:
                                        begin
                                            if (r_regs[w_decode_rs1_idx] != r_regs[w_decode_rs2_idx])
                                                begin
                                                    r_pc <= r_pc + { { 11 { w_decode_imm[19] } }, w_decode_imm, 1'b0 };
                                                end
                                        end

                                    // blt
                                    17'b???????1001100011:
                                        begin
                                            if ($signed(r_regs[w_decode_rs1_idx]) < $signed(r_regs[w_decode_rs2_idx]))
                                                begin
                                                    r_pc <= r_pc + { { 11 { w_decode_imm[19] } }, w_decode_imm, 1'b0 };
                                                end
                                        end

                                    // bge
                                    17'b???????1011100011:
                                        begin
                                            if ($signed(r_regs[w_decode_rs1_idx]) >= $signed(r_regs[w_decode_rs2_idx]))
                                                begin
                                                    r_pc <= r_pc + { { 11 { w_decode_imm[19] } }, w_decode_imm, 1'b0 };
                                                end
                                        end

                                    // bltu
                                    17'b???????1101100011:
                                        begin
                                            if (r_regs[w_decode_rs1_idx] < r_regs[w_decode_rs2_idx])
                                                begin
                                                    r_pc <= r_pc + { { 11 { w_decode_imm[19] } }, w_decode_imm, 1'b0 };
                                                end
                                        end

                                    // bgeu
                                    17'b???????1111100011:
                                        begin
                                            if (r_regs[w_decode_rs1_idx] >= r_regs[w_decode_rs2_idx])
                                                begin
                                                    r_pc <= r_pc + { { 11 { w_decode_imm[19] } }, w_decode_imm, 1'b0 };
                                                end
                                        end

                                    // lb
                                    17'b???????0000000011,
                                    // lbu
                                    17'b???????1000000011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    o_mem_write_en <= 0;
                                                    o_mem_req_size <= common::mem_req_size_byte;
                                                    o_mem_addr <= r_regs[w_decode_rs1_idx] + { { 12 { w_decode_imm[19] } }, w_decode_imm };

                                                    r_state <= cpu_state_memory_load_wait;
                                                end
                                        end

                                    // lh
                                    17'b???????0010000011,
                                    // lhu
                                    17'b???????1010000011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    o_mem_write_en <= 0;
                                                    o_mem_req_size <= common::mem_req_size_half;
                                                    o_mem_addr <= r_regs[w_decode_rs1_idx] + { { 12 { w_decode_imm[19] } }, w_decode_imm };

                                                    r_state <= cpu_state_memory_load_wait;
                                                end
                                        end

                                    // lw
                                    17'b???????0100000011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    o_mem_write_en <= 0;
                                                    o_mem_req_size <= common::mem_req_size_word;
                                                    o_mem_addr <= r_regs[w_decode_rs1_idx] + { { 12 { w_decode_imm[19] } }, w_decode_imm };

                                                    r_state <= cpu_state_memory_load_wait;
                                                end
                                        end

                                    // sb
                                    17'b???????0000100011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    o_mem_write_en <= 1;
                                                    o_mem_req_size <= common::mem_req_size_byte;
                                                    o_mem_addr <= r_regs[w_decode_rs1_idx] + { { 12 { w_decode_imm[19] } }, w_decode_imm };
                                                    o_mem_data <= r_regs[w_decode_rs2_idx];
                                                end
                                        end

                                    // sh
                                    17'b???????0010100011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    o_mem_write_en <= 1;
                                                    o_mem_req_size <= common::mem_req_size_half;
                                                    o_mem_addr <= r_regs[w_decode_rs1_idx] + { { 12 { w_decode_imm[19] } }, w_decode_imm };
                                                    o_mem_data <= r_regs[w_decode_rs2_idx];
                                                end
                                        end

                                    // sw
                                    17'b???????0100100011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    o_mem_write_en <= 1;
                                                    o_mem_req_size <= common::mem_req_size_word;
                                                    o_mem_addr <= r_regs[w_decode_rs1_idx] + { { 12 { w_decode_imm[19] } }, w_decode_imm };
                                                    o_mem_data <= r_regs[w_decode_rs2_idx];
                                                end
                                        end

                                    // addi
                                    17'b???????0000010011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_regs[w_decode_rs1_idx] + { { 12 { w_decode_imm[19] } }, w_decode_imm };
                                                end
                                        end

                                    // slti
                                    17'b???????0100010011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    if ($signed(r_regs[w_decode_rs1_idx]) < $signed({ { 12 { w_decode_imm[19] } }, w_decode_imm }))
                                                        begin
                                                            r_regs[w_decode_rd_idx] <= 1;
                                                        end
                                                    else
                                                        begin
                                                            r_regs[w_decode_rd_idx] <= 0;
                                                        end
                                                end
                                        end

                                    // sltiu
                                    17'b???????0110010011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    if (r_regs[w_decode_rs1_idx] < { { 12 { w_decode_imm[19] } }, w_decode_imm })
                                                        begin
                                                            r_regs[w_decode_rd_idx] <= 1;
                                                        end
                                                    else
                                                        begin
                                                            r_regs[w_decode_rd_idx] <= 0;
                                                        end
                                                end
                                        end

                                    // xori
                                    17'b???????1000010011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_regs[w_decode_rs1_idx] ^ { { 12 { w_decode_imm[19] } }, w_decode_imm };
                                                end
                                        end

                                    // ori
                                    17'b???????1100010011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_regs[w_decode_rs1_idx] | { { 12 { w_decode_imm[19] } }, w_decode_imm };
                                                end
                                        end

                                    // andi
                                    17'b???????1110010011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_regs[w_decode_rs1_idx] & { { 12 { w_decode_imm[19] } }, w_decode_imm };
                                                end
                                        end

                                    // slli
                                    17'b00000000010010011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_regs[w_decode_rs1_idx] << { { 12 { w_decode_imm[19] } }, w_decode_imm };
                                                end
                                        end

                                    // srli
                                    17'b00000001010010011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_regs[w_decode_rs1_idx] >> { { 12 { w_decode_imm[19] } }, w_decode_imm };
                                                end
                                        end

                                    // srai
                                    17'b01000001010010011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= $signed(r_regs[w_decode_rs1_idx]) >> $signed({ { 12 { w_decode_imm[19] } }, w_decode_imm });
                                                end
                                        end

                                    // add
                                    17'b00000000000110011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_regs[w_decode_rs1_idx] + r_regs[w_decode_rs2_idx];
                                                end
                                        end

                                    // sub
                                    17'b01000000000110011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_regs[w_decode_rs1_idx] - r_regs[w_decode_rs2_idx];
                                                end
                                        end

                                    // sll
                                    17'b00000000010110011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_regs[w_decode_rs1_idx] << r_regs[w_decode_rs2_idx];
                                                end
                                        end

                                    // slt
                                    17'b00000000100110011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    if ($signed(r_regs[w_decode_rs1_idx]) < $signed(r_regs[w_decode_rs2_idx]))
                                                        begin
                                                            r_regs[w_decode_rd_idx] <= 1;
                                                        end
                                                    else
                                                        begin
                                                            r_regs[w_decode_rd_idx] <= 0;
                                                        end
                                                end
                                        end

                                    // sltu
                                    17'b00000000110110011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    if (r_regs[w_decode_rs1_idx] < r_regs[w_decode_rs2_idx])
                                                        begin
                                                            r_regs[w_decode_rd_idx] <= 1;
                                                        end
                                                    else
                                                        begin
                                                            r_regs[w_decode_rd_idx] <= 0;
                                                        end
                                                end
                                        end

                                    // xor
                                    17'b00000001000110011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_regs[w_decode_rs1_idx] ^ r_regs[w_decode_rs2_idx];
                                                end
                                        end

                                    // srl
                                    17'b00000001010110011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_regs[w_decode_rs1_idx] >> r_regs[w_decode_rs2_idx];
                                                end
                                        end

                                    // sra
                                    17'b01000001010110011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= $signed(r_regs[w_decode_rs1_idx]) >> $signed(r_regs[w_decode_rs2_idx]);
                                                end
                                        end

                                    // or
                                    17'b00000001100110011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_regs[w_decode_rs1_idx] | r_regs[w_decode_rs2_idx];
                                                end
                                        end

                                    // and
                                    17'b00000001110110011:
                                        begin
                                            if (w_decode_rd_is_valid)
                                                begin
                                                    r_regs[w_decode_rd_idx] <= r_regs[w_decode_rs1_idx] & r_regs[w_decode_rs2_idx];
                                                end
                                        end

                                    // TODO: Unsupported Instructions
                                    //       fence
                                    //       fence.i
                                    //       ecall
                                    //       ebreak
                                    //       csrrw
                                    //       csrrs
                                    //       csrrc
                                    //       csrrwi
                                    //       csrrsi
                                    //       csrrci

                                endcase
                            end
                        else
                            begin
                                // Move to the halted state if we encounter an invalid instruction
                                r_state <= cpu_state_halt;
                            end
                    end
                cpu_state_memory_load_wait:
                    begin
                        // Wait a cycle for a memory load to occur
                        r_state <= cpu_state_memory_load_execute;
                    end
                cpu_state_memory_load_execute:
                    begin
                        casez({ w_decode_func, w_decode_op })

                            // lb
                            17'b???????0000000011:
                                begin
                                    r_regs[w_decode_rd_idx] <= { { 24 { i_mem_data[7] } }, i_mem_data[7:0] };
                                end

                            // lbu
                            17'b???????1000000011:
                                begin
                                    r_regs[w_decode_rd_idx] <= { 24'b0, i_mem_data[7:0] };
                                end

                            // lh
                            17'b???????0010000011:
                                begin
                                    r_regs[w_decode_rd_idx] <= { { 16 { i_mem_data[15] } }, i_mem_data[15:0] };
                                end

                            // lhu
                            17'b???????1010000011:
                                begin
                                    r_regs[w_decode_rd_idx] <= { 16'b0, i_mem_data[15:0] };
                                end

                            // lw
                            17'b???????0100000011:
                                begin
                                    r_regs[w_decode_rd_idx] <= i_mem_data[31:0];
                                end

                        endcase

                        r_state <= cpu_state_fetch;
                    end
                cpu_state_halt:
                    begin
                        // We stay in this state until reset
                        r_state <= cpu_state_halt;
                    end
            endcase
        end

endmodule
