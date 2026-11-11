/**
 * One refresh at a time. A request for the key already being read shares that reading; a request
 * for another key (another profile's usage) waits for it and is then read itself, never answered
 * with the other key's result.
 */
export function keyedRefresh<T>(read: (key: string) => Promise<T>): (key: string) => Promise<T> {
  let current: { key: string; run: Promise<T> } | undefined;
  return (key) => {
    if (current?.key === key) return current.run;
    const before = current ? current.run.then(() => undefined, () => undefined) : Promise.resolve();
    const mine = { key, run: before.then(() => read(key)) };
    current = mine;
    const done = () => {
      if (current === mine) current = undefined;
    };
    mine.run.then(done, done);
    return mine.run;
  };
}
