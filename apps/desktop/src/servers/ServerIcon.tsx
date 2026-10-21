import { serverInitials } from "./servers-store";

/** Ícone do servidor: a imagem (96 px) ou as iniciais do nome, no quadrado da barra de salas. */
export function ServerIcon(props: { name: string; icon: string | null; size?: number }) {
  const size = props.size ?? 48;
  return props.icon ? (
    <img
      src={props.icon}
      alt=""
      width={size}
      height={size}
      className="block object-cover"
      style={{ width: size, height: size }}
    />
  ) : (
    <span
      className="grid place-items-center bg-surface-3 font-semibold"
      style={{ width: size, height: size, fontSize: Math.round(size * 0.3) }}
    >
      {serverInitials(props.name)}
    </span>
  );
}
