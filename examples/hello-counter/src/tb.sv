// Self-checking testbench. Prints "PASS" on success, "FAIL: ..." otherwise.
//
// The check is "the counter increments after reset". We deliberately do
// not assert an exact post-reset count value, because simulator-specific
// scheduling can shift that by one cycle and the goal here is to verify
// that the kiln build+run pipeline works end-to-end, not to pin down the
// simulator event ordering.
module tb;
    logic       clk;
    logic       rst_n;
    logic [3:0] count;
    logic [3:0] sample_a;
    logic [3:0] sample_b;

    counter dut (.clk(clk), .rst_n(rst_n), .count(count));

    initial begin
        clk = 1'b0;
        forever #5 clk = ~clk;
    end

    initial begin
        rst_n = 1'b0;
        #20;
        rst_n = 1'b1;
        #50;
        sample_a = count;
        #50;
        sample_b = count;
        if (sample_b > sample_a) $display("PASS");
        else                     $display("FAIL: sample_a=%0d sample_b=%0d", sample_a, sample_b);
        $finish;
    end
endmodule
