// Demo module that triggers a width-trunc warning when slang's
// `-Wwidth-trunc` is on (Kiln.toml [lint] enables it).
module demo;
    logic [3:0] a;
    logic [7:0] b;
    initial begin
        b = 8'h7f;
        a = b;
    end
endmodule
