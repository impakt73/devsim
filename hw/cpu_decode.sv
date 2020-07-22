module cpu_decode
(
    input  logic [31:0] i_inst,

    output logic [6:0]  o_op,
    output logic [4:0]  o_rd,
    output logic [4:0]  o_rs1,
    output logic [4:0]  o_rs2,
    output logic [9:0]  o_func,
    output logic [19:0] o_imm,
    output logic        o_valid
);

// Opcode
assign o_op = i_inst[6:0];

// Destination register
assign o_rd = i_inst[11:7];

// Source register 1
assign o_rs1 = i_inst[19:15];

// Source register 2
assign o_rs2 = i_inst[24:20];

// Instruction function specifier
assign o_func = { i_inst[31:25], i_inst[14:12] };

// Intermediate helper wire values

wire [11:0] w_inst_i_imm;
assign w_inst_i_imm = { i_inst[31:20] };

wire [11:0] w_inst_s_imm;
assign w_inst_s_imm = { i_inst[31:25], i_inst[11:7] };

wire [11:0] w_inst_b_imm;
assign w_inst_b_imm = { i_inst[31], i_inst[7], i_inst[30:25], i_inst[11:8] };

wire [19:0] w_inst_u_imm;
assign w_inst_u_imm = { i_inst[31:12] };

wire [19:0] w_inst_j_imm;
assign w_inst_j_imm = { i_inst[31], i_inst[19:12], i_inst[20], i_inst[30:21] };

// Determine the immediate value encoded in the instruction
always_comb
    begin
        // Determine the instruction format type
        case (i_inst[6:0])
            // U type
            'b0110111,
            'b0010111:
                begin
                    o_imm = w_inst_u_imm[19:0];
                    o_valid = 1;
                end

            // J type
            'b1101111:
                begin
                    o_imm = w_inst_j_imm[19:0];
                    o_valid = 1;
                end

            // B type
            'b1100011:
                begin
                    o_imm = { { 8 { w_inst_b_imm[11] } }, w_inst_b_imm[11:0] };
                    o_valid = 1;
                end

            // S type
            'b0100011:
                begin
                    o_imm = { { 8 { w_inst_s_imm[11] } }, w_inst_s_imm[11:0] };
                    o_valid = 1;
                end

            // R type
            'b0110011:
                begin
                    // R type instructions never have immediate values
                    o_imm = 0;
                    o_valid = 1;
                end

            // I type
            'b1100111,
            'b0000011,
            'b0010011,
            'b0001111,
            'b1110011:
                begin
                    o_imm = { { 8 { w_inst_i_imm[11] } }, w_inst_i_imm[11:0] };
                    o_valid = 1;
                end

            // Invalid type
            default:
                begin
                    o_imm = 0;
                    o_valid = 0;
                end
        endcase   
    end

endmodule