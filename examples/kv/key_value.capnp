@0xe58a54d129477f35;

# A directory is a capability scoped to one namespace. Values and child
# directories are shared by all clients and live until the server exits.
interface Directory {
  open @0 (name :Text) -> (directory :Directory);
  put @1 (key :Text, value :Data) -> ();
  get @2 (key :Text) -> (found :Bool, value :Data);
  delete @3 (key :Text) -> (deleted :Bool);
}
