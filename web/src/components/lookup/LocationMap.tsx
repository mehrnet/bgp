import { createSignal, For, type JSX } from "solid-js";
import { formatCoord } from "../../lib/ip.ts";
import { Icon } from "../ui/Icon.tsx";

type MapMode = "osm" | "roadmap" | "satellite";

interface LocationMapProps {
  latitude: number;
  longitude: number;
  placeLabel: string;
}

const MODES: { id: MapMode; label: string }[] = [
  { id: "osm", label: "Street map" },
  { id: "roadmap", label: "Google map" },
  { id: "satellite", label: "Satellite" }
];

function buildMapUrl(mode: MapMode, lat: number, lng: number): string {
  if (mode === "osm") {
    const d = 0.08;
    const bbox = [lng - d, lat - d, lng + d, lat + d].map((n) => n.toFixed(5)).join(",");
    return `https://www.openstreetmap.org/export/embed.html?bbox=${bbox}&layer=mapnik&marker=${lat},${lng}`;
  }
  const typeParam = mode === "satellite" ? "k" : "m";
  return `https://maps.google.com/maps?q=${lat},${lng}&t=${typeParam}&z=12&ie=UTF8&output=embed`;
}

function MapControls(props: { mode: MapMode; lat: number; lng: number; onMode: (m: MapMode) => void }): JSX.Element {
  const streetViewUrl = () =>
    `https://www.google.com/maps/@?api=1&map_action=pano&viewpoint=${props.lat},${props.lng}`;

  return (
    <div class="location-map-controls">
      <For each={MODES}>
        {(item) => (
          <button
            type="button"
            class="map-mode-button"
            aria-pressed={props.mode === item.id}
            onClick={() => props.onMode(item.id)}
          >
            {item.label}
          </button>
        )}
      </For>
      <a class="map-external-link" href={streetViewUrl()} target="_blank" rel="noopener noreferrer">
        Open Street View ↗
      </a>
    </div>
  );
}

export function LocationMap(props: LocationMapProps): JSX.Element {
  const [mode, setMode] = createSignal<MapMode>("osm");
  const osmDirectUrl = () =>
    `https://www.openstreetmap.org/?mlat=${props.latitude}&mlon=${props.longitude}#map=12/${props.latitude}/${props.longitude}`;

  return (
    <section class="record-panel location-map-panel" aria-label="Geolocation map">
      <header class="record-panel-header location-map-header">
        <h3 class="record-heading">
          <Icon id="icon-map-pin" />
          <span>Location map</span>
        </h3>
        <MapControls mode={mode()} lat={props.latitude} lng={props.longitude} onMode={setMode} />
      </header>
      <div class="location-map-frame">
        <iframe
          class="location-map-iframe"
          title={`Map for ${props.placeLabel}`}
          src={buildMapUrl(mode(), props.latitude, props.longitude)}
          loading="lazy"
          referrerpolicy="no-referrer-when-downgrade"
        />
      </div>
      <footer class="location-map-footer">
        <span>
          Coordinates: <code>{`${formatCoord(props.latitude)}, ${formatCoord(props.longitude)}`}</code> · {props.placeLabel}
        </span>
        <a href={osmDirectUrl()} target="_blank" rel="noopener noreferrer">
          Open on OpenStreetMap ↗
        </a>
      </footer>
    </section>
  );
}
