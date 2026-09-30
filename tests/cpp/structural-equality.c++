#include <capnp/any.h>
#include <capnp/message.h>
#include <kj/debug.h>
#include <cstring>
#include <fstream>
#include <iostream>
#include <sstream>
#include <vector>

// Each case carries actual segment words, not a second implementation of the
// Rust fixture constructors. No schemas, canonicalization or capability table
// are needed to compare pointers; invalid table indices must remain unknown.
struct Wire {
  std::vector<kj::Array<capnp::word>> storage;
  std::vector<kj::ArrayPtr<const capnp::word>> segments;
  explicit Wire(std::istream& in) {
    unsigned count; KJ_REQUIRE(bool(in >> count));
    for (unsigned i=0;i<count;++i) {
      unsigned size; KJ_REQUIRE(bool(in >> size));
      auto words=kj::heapArray<capnp::word>(size);
      auto bytes=words.asBytes();
      for(unsigned j=0;j<size;++j) {
        uint64_t value; KJ_REQUIRE(bool(in >> value));
        for(unsigned k=0;k<8;++k) bytes[j*8+k]=value>>(k*8);
      }
      storage.push_back(kj::mv(words));
    }
    for(auto& words:storage) segments.push_back(words.asPtr());
  }
};
int main(int argc,char** argv) {
  KJ_REQUIRE(argc==2);
  std::ifstream input(argv[1]); KJ_REQUIRE(input.good());
  std::string line;
  while(std::getline(input,line)) {
    std::istringstream in(line); unsigned mode, depth; uint64_t limit;
    KJ_REQUIRE(bool(in >> mode >> depth >> limit));
    Wire left(in),right(in);
    capnp::ReaderOptions options;
    options.nestingLimit=depth; options.traversalLimitInWords=limit;
    try {
      capnp::SegmentArrayMessageReader l(kj::arrayPtr(left.segments.data(),left.segments.size()),options);
      capnp::SegmentArrayMessageReader r(kj::arrayPtr(right.segments.data(),right.segments.size()),options);
      auto a=l.getRoot<capnp::AnyPointer>(), b=r.getRoot<capnp::AnyPointer>();
      auto result=mode==0?a.equals(b):mode==1?a.getAs<capnp::AnyStruct>().equals(b.getAs<capnp::AnyStruct>()):
          a.getAs<capnp::AnyList>().equals(b.getAs<capnp::AnyList>());
      switch(result) {
        case capnp::Equality::EQUAL:std::cout<<1;break;
        case capnp::Equality::NOT_EQUAL:std::cout<<2;break;
        case capnp::Equality::UNKNOWN_CONTAINS_CAPS:std::cout<<3;break;
      }
    } catch(kj::Exception& e) {
      std::cout<<4;
    }
    std::cout<<'\n';
  }
}
