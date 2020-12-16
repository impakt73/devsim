module cpu_inst_fetch
(
    // Standard Signals
    input  bit        i_clk,
    input  bit        i_rst,

    // Control Signals
    input  bit [31:0] i_jmp_pc,
    input  bit        i_jmp_en,
    input  bit        i_read_en,
    input  bit        i_ready,

    // Memory Interface
    output bit [31:0] o_mem_addr,
    input  bit        i_mem_ready,
    input  bit [31:0] i_mem_data,
    input  bit        i_mem_valid,

    // Instruction Data Out
    output bit [31:0] o_inst_data,
    output bit        o_inst_data_valid
);

// Current program counter
bit [31:0] r_pc;

// Flag indicating that we're currently waiting on an outstanding memory request
bit r_wait_for_mem;

// The next program counter is typically the current value plus four bytes unless there's a jump occuring
bit [31:0] w_next_pc;
assign w_next_pc = i_jmp_en ? i_jmp_pc : r_pc + 4;

// We always request the memory stored at the current address in the program counter
assign o_mem_addr = r_pc;

// We return the instruction data directly from the memory system immediately once it's valid
assign o_inst_data       = i_mem_data;
assign o_inst_data_valid = i_mem_valid;

always_ff @ (posedge i_clk)
    if (i_rst)
        begin
            r_pc           <= 0;
            r_wait_for_mem <= 0;
        end
    else
        begin
            if (r_wait_for_mem == 0)
                begin
                    // We're waiting to request a new instruction from the memory system

                    if (i_mem_ready)
                        begin
                            // The memory system is ready and has received our pc address so now we need to
                            // wait for it to return the data we requested
                            r_wait_for_mem <= 1;
                        end
                end
            else
                begin
                    if (i_mem_valid)
                        begin
                            // The memory request has been completed
                            // We can now request the data for the next instruction
                            r_wait_for_mem <= 0;
                            r_pc           <= w_next_pc;
                        end
                end
        end

endmodule
