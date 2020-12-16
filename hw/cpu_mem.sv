module cpu_mem
(
    // Standard Signals
    input  bit        i_clk,
    input  bit        i_rst,

    // Memory Interface
    input  bit        i_mem_ready,
    input  bit        i_mem_valid,

    output bit        o_mem_write_en,
    output bit [31:0] o_mem_addr,
    output bit [31:0] o_mem_data,

    input  bit [31:0] i_mem_data,

    // Control Signals
    input  bit        i_write_en,

    // Address
    input  bit [31:0] i_addr,

    // Stores
    input  bit [31:0] i_data,

    // Loads
    output bit [31:0] o_data,

    output bit        o_ready
);

// Flag indicating that we're currently waiting on an outstanding memory request
bit r_wait_for_mem;

assign o_mem_write_en = i_write_en;
assign o_mem_addr     = i_addr;
assign o_mem_data     = i_data;
assign o_data         = ((r_wait_for_mem != 0) && i_mem_ready) ? i_mem_data : 0;
assign o_ready        = ((r_wait_for_mem == 0) && i_mem_ready);

always_ff @ (posedge i_clk)
    if (i_rst)
        r_wait_for_mem <= 0;
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
                    if (i_mem_ready)
                        begin
                            // The memory request has been completed
                            // We can now request the data for the next instruction
                            r_wait_for_mem <= 0;
                        end
                end
        end

endmodule
