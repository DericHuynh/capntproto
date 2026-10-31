@0xf298c20a533ea9d1;
interface Greeting {
  open @0 (prefix :Text) -> (session :Session);
}
interface Session {
  echo @0 (value :Text) -> (value :Text);
}
