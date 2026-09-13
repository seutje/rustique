import { useEffect, useState } from "react";
import { connectMidi, listenOsc, queryLiveInputValues, queryMidiPorts, type LiveInputValue } from "./native";

export function LiveInput() {
  const [ports, setPorts] = useState<string[]>([]);
  const [port, setPort] = useState(0);
  const [address, setAddress] = useState("127.0.0.1:9000");
  const [values, setValues] = useState<LiveInputValue[]>([]);
  const [error, setError] = useState("");
  const run = async (action: () => Promise<unknown>) => { try { setError(""); await action(); } catch (reason) { setError(String(reason)); } };
  useEffect(() => { const timer = window.setInterval(() => void queryLiveInputValues().then(setValues).catch(() => undefined), 250); return () => window.clearInterval(timer); }, []);
  return <><h3>Live input</h3><button onClick={() => void run(async () => setPorts(await queryMidiPorts()))}>Scan MIDI</button>{ports.length > 0 && <><select value={port} onChange={(event) => setPort(Number(event.target.value))}>{ports.map((name,index) => <option value={index} key={`${name}-${index}`}>{name}</option>)}</select><button onClick={() => void run(() => connectMidi(port))}>Connect MIDI</button></>}<label>OSC listen address</label><input value={address} onChange={(event) => setAddress(event.target.value)}/><button onClick={() => void run(() => listenOsc(address))}>Listen for OSC</button>{values.map((input) => <div className="live-input" key={input.source}><span>{input.source}</span><output>{input.value.toFixed(3)}</output></div>)}{error && <p className="error">{error}</p>}</>;
}
