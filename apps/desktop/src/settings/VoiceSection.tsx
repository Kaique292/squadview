import { useEffect } from "react";
import { Switch } from "../ui/Switch";
import { VolumeSlider } from "../voice/VoiceButtons";
import { type AudioDevice, useVoiceStore } from "../voice/voice-store";

/** Voz: microfone, saída, volumes, nível e isolamento de voz (como "Voz e vídeo" do Discord). */
export function VoiceSection() {
  const { prefs, setPrefs, devices, loadDevices, level } = useVoiceStore();
  const speaking = useVoiceStore((s) => !!s.speaking.me);
  useEffect(() => {
    void loadDevices();
  }, [loadDevices]);

  const bars = 32;
  const lit = Math.round(Math.min(1, level * 1.6) * bars);
  return (
    <section aria-labelledby="voz" className="mt-5 flex flex-col gap-3 text-left">
      <h3 id="voz" className="text-xs font-semibold tracking-wide text-text-faint uppercase">
        Voz
      </h3>
      <div className="flex flex-col gap-3 rounded-md bg-surface-1 p-3">
        <div className="grid grid-cols-2 gap-3">
          <DeviceSelect
            label="Microfone"
            devices={devices.inputs}
            value={prefs.inputDevice}
            onChange={(id) => void setPrefs({ inputDevice: id })}
          />
          <DeviceSelect
            label="Saída"
            devices={devices.outputs}
            value={prefs.outputDevice}
            onChange={(id) => void setPrefs({ outputDevice: id })}
          />
        </div>
        <div className="grid grid-cols-2 gap-3">
          <VolumeSlider
            label="Volume do microfone"
            value={prefs.inputVolume}
            onChange={(v) => void setPrefs({ inputVolume: v })}
          />
          <VolumeSlider
            label="Volume da saída"
            value={prefs.outputVolume}
            onChange={(v) => void setPrefs({ outputVolume: v })}
          />
        </div>
        <div>
          <p className="mb-1.5 text-sm font-medium">Nível do microfone</p>
          <div
            role="meter"
            aria-label="Nível do microfone"
            aria-valuenow={Math.round(level * 100)}
            className="flex gap-0.5"
          >
            {Array.from({ length: bars }, (_, i) => (
              <span
                key={i}
                className={`h-3 flex-1 rounded-sm ${i < lit ? (speaking ? "bg-online" : "bg-text-muted") : "bg-surface-3"}`}
              />
            ))}
          </div>
          <p className="mt-1 text-xs text-text-faint">Entre numa sala e fale: fica verde quando a voz está indo.</p>
        </div>
        <Switch
          label="Isolamento de voz"
          hint="Corta ruído de fundo, como teclado e ventilador. Roda no seu PC."
          checked={prefs.noiseSuppression}
          onChange={(v) => void setPrefs({ noiseSuppression: v })}
        />
        <p className="text-xs text-text-faint">
          Use fone de ouvido: com caixa de som, a galera pode ouvir o próprio eco (o cancelamento de eco chega numa
          próxima versão).
        </p>
      </div>
    </section>
  );
}

function DeviceSelect(props: {
  label: string;
  devices: AudioDevice[];
  value: string | null;
  onChange: (id: string | null) => void;
}) {
  return (
    <label className="flex min-w-0 flex-col gap-1.5">
      <span className="text-sm font-medium">{props.label}</span>
      <select
        value={props.value ?? ""}
        onChange={(e) => props.onChange(e.target.value || null)}
        className="w-full truncate rounded-md bg-surface-0 px-2 py-2 text-sm text-text focus:ring-2 focus:ring-brand focus:outline-none"
      >
        <option value="">Padrão do Windows</option>
        {props.devices.map((d) => (
          <option key={d.id} value={d.id}>
            {d.name}
          </option>
        ))}
      </select>
    </label>
  );
}
