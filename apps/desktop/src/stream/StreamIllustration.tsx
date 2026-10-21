/** Ilustração do topo do modal de transmitir: a galera assistindo a tela de um amigo. Arte própria do SquadView. */
export function StreamIllustration() {
  return (
    <svg width="280" height="128" viewBox="0 0 280 128" fill="none">
      {/* brilhos */}
      <path d="M38 30l3 7 7 3-7 3-3 7-3-7-7-3 7-3z" fill="#f2f3f5" opacity=".85" />
      <path d="M246 18l2 5 5 2-5 2-2 5-2-5-5-2 5-2z" fill="#f2f3f5" opacity=".7" />
      <circle cx="226" cy="52" r="3" fill="#9478ff" />
      <circle cx="58" cy="64" r="2.5" fill="#23a55a" />

      {/* tela ao vivo */}
      <rect x="82" y="8" width="116" height="74" rx="12" fill="#2e3035" stroke="#6e4ff6" strokeWidth="4" />
      <rect x="92" y="18" width="96" height="54" rx="6" fill="url(#sv-ilustra-tela)" />
      <path d="M133 34l18 11-18 11z" fill="#fff" />
      <rect x="95" y="22" width="34" height="11" rx="5.5" fill="#f23f43" />
      <text
        x="112"
        y="30.2"
        textAnchor="middle"
        fontSize="6.5"
        fontWeight="700"
        fill="#fff"
        fontFamily="Segoe UI, sans-serif"
      >
        AO VIVO
      </text>

      {/* sofá */}
      <rect x="56" y="96" width="168" height="26" rx="13" fill="#5b3be0" />
      <rect x="48" y="88" width="24" height="34" rx="12" fill="#6e4ff6" />
      <rect x="208" y="88" width="24" height="34" rx="12" fill="#6e4ff6" />

      {/* a galera */}
      <circle cx="104" cy="86" r="12" fill="#f0b232" />
      <rect x="90" y="96" width="28" height="18" rx="9" fill="#f0b232" />
      <circle cx="140" cy="82" r="13" fill="#23a55a" />
      <rect x="125" y="93" width="30" height="21" rx="10" fill="#23a55a" />
      <circle cx="176" cy="86" r="12" fill="#3ba0ff" />
      <rect x="162" y="96" width="28" height="18" rx="9" fill="#3ba0ff" />

      <defs>
        <linearGradient id="sv-ilustra-tela" x1="92" y1="18" x2="188" y2="72" gradientUnits="userSpaceOnUse">
          <stop stopColor="#1f3b5c" />
          <stop offset=".6" stopColor="#3f2a5c" />
          <stop offset="1" stopColor="#5c3a1f" />
        </linearGradient>
      </defs>
    </svg>
  );
}
