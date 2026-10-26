@0xe58a54d129477f35;

# Each result can be called before the preceding response arrives.
interface Node {
  value @0 () -> (depth :UInt32);
  child @1 () -> (node :Node);
}
