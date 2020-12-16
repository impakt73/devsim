module cpu_regfile
(
    // Standard Signals
    input  bit        i_clk,

    // Write Port
    input bit         i_reg_write_en,
    input bit [4:0]   i_reg_write_idx,
    input bit [31:0]  i_reg_write_data,

    // Read Port A
    input  bit [4:0]  i_reg_read_idx_a,
    output bit [31:0] o_reg_read_data_a,

    // Read Port B
    input  bit [4:0]  i_reg_read_idx_b,
    output bit [31:0] o_reg_read_data_b
);

// Register data
bit [31:0] r_regs[30:0];

// Return the register data for port A unless register zero is requested
assign o_reg_read_data_a = (i_reg_read_idx_a != 0) ? r_regs[i_reg_read_idx_a] : 0;

// Return the register data for port A unless register zero is requested
assign o_reg_read_data_b = (i_reg_read_idx_b != 0) ? r_regs[i_reg_read_idx_b] : 0;

always_ff @ (posedge i_clk)
    // Write the provided data into the target register as long as it's not register zero
    if (i_reg_write_en && (i_reg_write_idx != 0))
        r_regs[i_reg_write_idx] <= i_reg_write_data;
endmodule
