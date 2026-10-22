/** A chave da nossa live entre os vídeos (não colide com o id de ninguém). */
export const OWN_VIDEO = "transmissao:minha";

/** A chave de um vídeo a partir do `source` que o Rust manda (`null`: a nossa live). */
export function videoKey(source: string | null | undefined): string {
  return source ?? OWN_VIDEO;
}
