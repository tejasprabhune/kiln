// 4-bit up counter with synchronous-deassert async-assert reset.
module counter (
    input  logic       clk,
    input  logic       rst_n,
    output logic [3:0] count
);
    always_ff @(posedge clk or negedge rst_n) begin
        if (!rst_n) count <= '0;
        else        count <= count + 4'd1;
    end
endmodule
