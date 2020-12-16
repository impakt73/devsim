module mem
#(
    parameter NUM_BYTES = 4 * 1024 * 1024
)
(
    input logic         i_clk,
    input logic         i_rst,

    // Control Flags
    output logic        o_ready,
    output logic        o_valid,

    input  logic        i_en,
    input  logic        i_write_en,

    // I/O
    output logic [31:0] o_data,

    input  logic [31:0] i_data,

    input  logic [31:0] i_addr,
    input  logic [3:0]  i_mask
);

localparam NUM_ADDR_BITS = $clog2(NUM_BYTES);

reg [31:0] r_memory[NUM_BYTES-1:0];
reg [31:0] r_output_data;
reg r_output_data_valid;

assign o_ready = 1;
assign o_valid = r_output_data_valid;
assign o_data = r_output_data_valid ? r_output_data : 0;

assign w_addr = i_addr[NUM_ADDR_BITS-1:0];

assign w_in_data = { (i_mask[0] ? i_data[7:0]   : r_memory[w_addr][7:0]),
                     (i_mask[1] ? i_data[15:8]  : r_memory[w_addr][15:8]),
                     (i_mask[2] ? i_data[23:16] : r_memory[w_addr][23:16]),
                     (i_mask[3] ? i_data[31:24] : r_memory[w_addr][31:24]) };

assign w_out_data = { (i_mask[0] ? r_memory[w_addr][7:0]   : 8'b0),
                      (i_mask[1] ? r_memory[w_addr][15:8]  : 8'b0),
                      (i_mask[2] ? r_memory[w_addr][23:16] : 8'b0),
                      (i_mask[3] ? r_memory[w_addr][31:24] : 8'b0) };

always @ (posedge i_clk)
    if (i_rst)
        begin
            r_output_data       <= 0;
            r_output_data_valid <= 0;
        end
    else
        begin
            r_output_data_valid <= 0;

            if (i_en)
                begin
                    if (i_write_en)
                        begin
                            r_memory[w_addr] <= w_in_data;
                        end
                    else
                        begin
                            r_output_data       <= w_out_data;
                            r_output_data_valid <= 1;
                        end
                end
        end

endmodule