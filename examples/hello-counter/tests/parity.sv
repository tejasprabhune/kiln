// Parity test: drives reset, runs a few cycles, asserts that count
// toggles between odd and even values. With `KILN_TRACE` defined,
// dumps an FST trace.
module parity;
    logic       clk;
    logic       rst_n;
    logic [3:0] count;
    logic       even_seen;
    logic       odd_seen;

    counter dut (.clk(clk), .rst_n(rst_n), .count(count));

    initial begin
        clk = 1'b0;
        forever #5 clk = ~clk;
    end

`ifdef KILN_TRACE
    initial begin
        $dumpfile("parity.fst");
        $dumpvars(0, parity);
    end
`endif

    initial begin
        even_seen = 1'b0;
        odd_seen  = 1'b0;
        rst_n     = 1'b0;
        #20;
        rst_n = 1'b1;
        repeat (8) begin
            #10;
            if (count[0]) odd_seen  = 1'b1;
            else          even_seen = 1'b1;
        end
        if (even_seen && odd_seen) $display("PASS");
        else                       $display("FAIL: parity not exercised");
        $finish;
    end
endmodule
