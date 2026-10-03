#include <capnp/compat/byte-stream.h>
#include <capnp/compat/http-over-capnp.h>
#include <capnp/rpc-twoparty.h>
#include <capnp/compat/json-rpc.h>
#include <capnp/schema-parser.h>
#include <capnp/dynamic.h>
#include <kj/filesystem.h>
#include <kj/async-io.h>
#include <kj/compat/http.h>
#include <iostream>
#include <fstream>
#include <vector>

class Output final: public capnp::ExplicitEndOutputStream {
public:
  Output(std::string path): path(path) {}
  kj::Promise<void> write(kj::ArrayPtr<const kj::byte> data) override { bytes.insert(bytes.end(),data.begin(),data.end());return kj::READY_NOW; }
  kj::Promise<void> write(kj::ArrayPtr<const kj::ArrayPtr<const kj::byte>> pieces) override {for(auto p:pieces){bytes.insert(bytes.end(),p.begin(),p.end());}return kj::READY_NOW;}
  kj::Promise<void> whenWriteDisconnected() override {return kj::NEVER_DONE;}
  kj::Promise<void> end() override {std::ofstream out(path,std::ios::binary);out.write(reinterpret_cast<char*>(bytes.data()),bytes.size());return kj::READY_NOW;}
private:
  std::string path;std::vector<kj::byte> bytes;
};
class Echo final:public kj::HttpService {
public:
  Echo(const kj::HttpHeaderTable& table):table(table){}
  kj::Promise<void> request(kj::HttpMethod method,kj::StringPtr url,const kj::HttpHeaders&,kj::AsyncInputStream& input,Response& response) override {
    if(url=="/ws") {
      kj::HttpHeaders headers(table);auto ws=response.acceptWebSocket(headers);
      auto message=co_await ws->receive();
      KJ_REQUIRE(message.is<kj::Array<kj::byte>>());
      co_await ws->send(message.get<kj::Array<kj::byte>>());
      co_await ws->close(1000,"done");
    } else {
      auto bytes=co_await input.readAllBytes();
      kj::HttpHeaders headers(table);headers.set(kj::HttpHeaderId::CONTENT_TYPE,"application/octet-stream");
      auto output=response.send(200,"OK",headers,bytes.size());
      co_await output->write(bytes);
    }
  }
  kj::Promise<void> connect(kj::StringPtr,const kj::HttpHeaders&,kj::AsyncIoStream& stream,ConnectResponse& response,kj::HttpConnectSettings) override {
    kj::HttpHeaders headers(table);response.accept(200,"OK",headers);
    auto bytes=co_await stream.readAllBytes();co_await stream.write(bytes);stream.shutdownWrite();
  }
private:
  const kj::HttpHeaderTable& table;
};
class Calculator final: public capnp::DynamicCapability::Server {
public:
  Calculator(capnp::InterfaceSchema schema):Server(schema){}
  kj::Promise<void> call(capnp::InterfaceSchema::Method method,capnp::CallContext<capnp::DynamicStruct,capnp::DynamicStruct> context) override {
    auto name=method.getProto().getName();
    if(name=="add")context.getResults().set("value",context.getParams().get("x").as<int32_t>()+context.getParams().get("y").as<int32_t>());
    else if(name=="fail")return KJ_EXCEPTION(FAILED,"expected C++ failure");
    return kj::READY_NOW;
  }
};
int main(int argc,char** argv) {
  try {
    KJ_REQUIRE(argc==3);
    auto io=kj::setupAsyncIo();
    capnp::ByteStreamFactory streams;
    kj::HttpHeaderTable::Builder headerBuilder;
    capnp::HttpOverCapnpFactory http(streams,headerBuilder,capnp::HttpOverCapnpFactory::LEVEL_2);
    auto headers=headerBuilder.build();
    capnp::Capability::Client bootstrap=nullptr;
    if(std::string(argv[1])=="byte")bootstrap=streams.kjToCapnp(kj::heap<Output>(argv[2]));
    else bootstrap=http.kjToCapnp(kj::heap<Echo>(*headers));
    capnp::TwoPartyServer server(bootstrap);
    auto address=io.provider->getNetwork().parseAddress("127.0.0.1",0).wait(io.waitScope);
    auto listener=address->listen();std::cout<<listener->getPort()<<std::endl;
    auto connection=listener->accept().wait(io.waitScope);
    if(std::string(argv[1])=="json") {
      auto fs=kj::newDiskFilesystem();auto imports=fs->getCurrent().openSubdir(kj::Path::parse("vendor/capnproto/c++/src"));const kj::ReadableDirectory* paths[]={imports.get()};
      capnp::SchemaParser parser;
      auto schema=parser.parseFromDirectory(fs->getCurrent(),kj::Path::parse("crates/capntproto-compat/tests/compat.capnp"),paths).getNested("Calculator").asInterface();
      capnp::JsonRpc::ContentLengthTransport transport(*connection);
      capnp::JsonRpc json(transport,kj::heap<Calculator>(schema));json.onError().wait(io.waitScope);
    } else {server.accept(*connection).wait(io.waitScope);}
    return 0;
  }catch(const kj::Exception& e){if(e.getType()==kj::Exception::Type::DISCONNECTED)return 0;std::cerr<<e.getDescription().cStr()<<std::endl;return 2;}
}
