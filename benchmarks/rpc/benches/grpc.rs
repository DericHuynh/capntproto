fn main() -> Result<(), Box<dyn std::error::Error>> {
    reproto_rpc_bench::run(Some("grpc"))
}
