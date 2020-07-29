`ifndef PKG_COMMON
`define PKG_COMMON

package common;

typedef enum bit[1:0]
{
    mem_req_size_byte,
    mem_req_size_half,
    mem_req_size_word
} mem_req_size;

typedef enum bit[3:0]
{
    cmd_id_reset,
    cmd_id_read,
    cmd_id_write
} cmd_id;

endpackage

`endif