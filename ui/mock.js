window.__MOCK__ = (() => {
  const rows = [
    {path: 'C:/r/Tuonela LE (115).SC2Replay', name: 'Tuonela LE (115).SC2Replay', modified_secs: 1758000000, size: 227146},
    {path: 'C:/r/Ley Lines LE (3).SC2Replay', name: 'Ley Lines LE (3).SC2Replay', modified_secs: 1757900000, size: 180000},
    {path: 'C:/r/broken.SC2Replay', name: 'broken.SC2Replay', modified_secs: 1757800000, size: 12},
  ];
  const chart = name => '<!doctype html><html><body style="font-family:system-ui;padding:24px"><h1>Mock chart</h1><p>' + name + '</p></body></html>';
  return {
    invoke: async (cmd, args) => {
      if (cmd === 'list_replays') return rows;
      if (cmd === 'roots') return ['C:/r'];
      if (cmd === 'add_root') { return ['C:/r', args.path]; }
      if (cmd === 'chart_html') {
        if (args.path.endsWith('broken.SC2Replay')) throw 'could not parse replay header: not a StarCraft II replay';
        return chart(args.path.split('/').pop());
      }
      throw 'unknown command ' + cmd;
    },
    listen: async () => () => {},
    open: async opts => opts && opts.directory ? 'D:/more' : 'C:/r/Ley Lines LE (3).SC2Replay',
  };
})();
