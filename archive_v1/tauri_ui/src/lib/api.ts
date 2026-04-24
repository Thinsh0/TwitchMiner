import axios from 'axios';
import { Command } from '@tauri-apps/plugin-shell';

// Python Server URL (hardcoded for now, or from config)
const API_URL = "http://localhost:36969";

export const api = axios.create({
    baseURL: API_URL,
});

export interface MinerStatus {
    status: string;
    streamer: string | null;
    streamer_image_url: string | null;
    drop: string | null;
    progress: number;
    campaign_progress: number;
    image_url: string | null;
    current_points: number | string;
    authenticated: boolean;
}

export const getStatus = async (): Promise<MinerStatus> => {
    const res = await api.get<MinerStatus>("/status");
    return res.data;
};

export const startMiner = async () => {
    return api.post("/start");
}

export const stopMiner = async () => {
    return api.post("/stop");
}

export const getInventory = async () => {
    const res = await api.get("/inventory");
    return res.data;
}

export const searchGames = async (query: string) => {
    const res = await api.get(`/search?query=${encodeURIComponent(query)}`);
    return res.data;
}

export const updatePriorities = async (games: string[], streamers: string[]) => {
    return api.post("/priorities", { games, streamers });
}

export const getConfig = async () => {
    const res = await api.get("/config");
    return res.data;
}

export const startAuth = async () => {
    const res = await api.post("/auth/start");
    return res.data;
}

export const pollAuth = async (device_code: string, interval: number) => {
    const res = await api.post("/auth/poll_blocking", { device_code, interval });
    return res.data;
}

export const getBonusStatus = async () => {
    const res = await api.get("/bonus/status");
    return res.data;
}

export const updateSetting = async (section: string, key: string, value: any) => {
    return api.post("/settings/update", { section, key, value });
}

let sidecarRunning = false;

export const startSidecar = async () => {
    if (sidecarRunning) {
        console.log("Sidecar already running (or starting). Skipping.");
        return;
    }
    sidecarRunning = true;

    try {
        const command = Command.sidecar("twitch-miner-sidecar");

        command.stdout.on('data', line => console.log(`[Sidecar] ${line}`));
        command.stderr.on('data', line => console.error(`[Sidecar Error] ${line}`));
        command.on('close', data => console.log(`[Sidecar] Exited with code ${data.code}`));
        command.on('error', error => console.error(`[Sidecar] Error: ${error}`));

        const child = await command.spawn();
        console.log("Sidecar spawned with PID:", child.pid);
        return child;
    } catch (e) {
        console.error("Failed to spawn sidecar:", e);
        sidecarRunning = false;
    }
}
