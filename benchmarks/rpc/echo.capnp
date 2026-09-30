@0x83d4a973dcdf17d1;
interface Echo {
  echo @0 (sequence :UInt64, payload :Data) -> (sequence :UInt64, payload :Data);
}
