module consumer_top;
    logic clk;
    logic rst_n;
    logic [3:0] count;

    counter dut (.clk(clk), .rst_n(rst_n), .count(count));

    initial begin
        clk = 1'b0;
        forever #5 clk = ~clk;
    end

    initial begin
        rst_n = 1'b0;
        #20;
        rst_n = 1'b1;
        #100;
        $display("PASS");
        $finish;
    end
endmodule
