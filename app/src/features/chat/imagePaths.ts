/** Keep paths as text too: a missing or refused image must still be readable. */
export function imagePaths(text: string): string[] {
  text = text.replace(/https?:\/\/[^\s<>()]+/gi, "");
  const paths: string[] = [];
  const add = (path: string) => { path = path.trim().replace(/^<|>$/g, ""); if (!/^(https?:|\/\/|\\\\)/i.test(path) && !paths.includes(path)) paths.push(path); };
  for (const match of text.matchAll(/!?\[[^\]]*\]\((<?[^\r\n)]*?\.(?:png|jpe?g|gif|webp)>?)\)/gi)) add(match[1]);
  for (const match of text.matchAll(/`([^`\r\n]+\.(?:png|jpe?g|gif|webp))`/gi)) add(match[1]);
  for (const match of text.matchAll(/[A-Za-z]:[\\/][^\r\n<>"'`()[\]]+?\.(?:png|jpe?g|gif|webp)/gi)) add(match[0]);
  for (const match of text.matchAll(/(?:[A-Za-z]:[\\/]|\/)(?:[^\s<>"'`()[\]]+[\\/])*[^\s<>"'`()[\]]+\.(?:png|jpe?g|gif|webp)/gi)) add(match[0]);
  for (const match of text.matchAll(/(?:^|\s)((?:\.\.?[\\/])?(?:[\w.-]+[\\/])+[\w.-]+\.(?:png|jpe?g|gif|webp))(?=$|\s|[.,!?])/gim)) add(match[1]);
  return paths.slice(0, 20);
}
