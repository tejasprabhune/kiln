module consumer
    import my_pkg::*;
(
    input  data_t in_data,
    output data_t out_data
);
    assign out_data = in_data;
endmodule
