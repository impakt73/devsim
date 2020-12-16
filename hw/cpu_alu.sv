module cpu_alu
(
    // Operation
    input  logic [3:0]  i_op,

    // Parameter A
    input  logic [31:0] i_a,

    // Parameter B
    input  logic [31:0] i_b,

    // Result
    output logic [31:0] o_y
);

always_comb
    // Perform the requested ALU operation
    case (i_op)
        // ADD
        'b0000:
            o_y = $signed(i_a) + $signed(i_b);
        // SUB
        'b1000:
            o_y = $signed(i_a) - $signed(i_b);
        // SLL
        'b0001:
            o_y = i_a << i_b;
        // SLT
        'b0010:
            o_y = ($signed(i_a) < $signed(i_b)) ? 1 : 0;
        // SLTU
        'b0011:
            o_y = (i_a < i_b) ? 1 : 0;
        // XOR
        'b0100:
            o_y = i_a ^ i_b;
        // SRL
        'b0101:
            o_y = i_a >> i_b;
        // SRA
        'b1101:
            o_y = $signed(i_a) >>> $signed(i_b);
        // OR
        'b0110:
            o_y = i_a | i_b;
        // AND
        'b0111:
            o_y = i_a & i_b;
        // Invalid operation
        default:
            o_y = 0;
    endcase   

endmodule