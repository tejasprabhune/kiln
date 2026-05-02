// Smoke test: bring up reset, sample twice, verify the counter advances.
// Top module is `smoke` (filename stem).
module smoke;
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
        else                     $display("FAIL: a=%0d b=%0d", sample_a, sample_b);
        $finish;
    end
endmodule
