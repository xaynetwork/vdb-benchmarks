final: prev: {
  hdf5-merged = final.symlinkJoin {
    name = "hdf5-merged";
    paths = with final; [
      hdf5_1_10
      hdf5_1_10.dev
    ];
  };
}
