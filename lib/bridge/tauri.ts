/**
 * TauriBridge — the real path (§9.4): written in full now so Rust has a
 * concrete caller to build against. Each method maps to a #[tauri::command]
 * in src-tauri/src/commands/; until written, calls reject — no silent mock fallback.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type {
  ActionItem,
  Edge,
  Entry,
  ModelInfo,
  Question,
  Settings,
  Span,
  SystemProfile,
} from '@/lib/types';
import type {
  Bridge,
  CorpusImport,
  ImportMode,
  NewEntryDraft,
  SearchHit,
  Unsubscribe,
} from './index';

/**
 * Tauri event listeners register asynchronously, so the unsubscribe function
 * has to survive being called before registration completes.
 */
function subscribe<T>(event: string, cb: (payload: T) => void): Unsubscribe {
  let stop: (() => void) | null = null;
  let cancelled = false;

  void listen<T>(event, (e) => cb(e.payload)).then((unlisten) => {
    if (cancelled) unlisten();
    else stop = unlisten;
  });

  return () => {
    cancelled = true;
    stop?.();
  };
}

export class TauriBridge implements Bridge {
  readonly kind = 'tauri' as const;

  // -- corpus -------------------------------------------------------------

  listEntries(): Promise<Entry[]> {
    return invoke('list_entries');
  }

  getEntry(id: string): Promise<Entry | null> {
    return invoke('get_entry', { id });
  }

  listChildren(entryId: string): Promise<Entry[]> {
    return invoke('list_children', { entryId });
  }

  listEdges(): Promise<Edge[]> {
    return invoke('list_edges');
  }

  createEntry(draft: NewEntryDraft): Promise<Entry> {
    return invoke('create_entry', { draft });
  }

  moveEntry(id: string, x: number, y: number): Promise<Entry> {
    return invoke('move_entry', { id, x, y });
  }

  deleteEntry(id: string): Promise<void> {
    return invoke('delete_entry', { id });
  }

  // -- search ---------------------------------------------------------------

  searchEntries(query: string): Promise<SearchHit[]> {
    return invoke('search_entries', { query });
  }

  // -- capture ------------------------------------------------------------

  startRecording(): Promise<void> {
    return invoke('start_recording');
  }

  stopRecording(parentEdge: string | null = null): Promise<Entry> {
    return invoke('stop_recording', { parentEdge });
  }

  discardRecording(): Promise<void> {
    return invoke('discard_recording');
  }

  undoDiscard(): Promise<Entry | null> {
    return invoke('undo_discard');
  }

  /** cpal capture emits these; the equalizer is the only animation in the app (§8). */
  onAmplitude(cb: (level: number) => void): Unsubscribe {
    return subscribe<number>('capture://amplitude', cb);
  }

  // -- enrichment ---------------------------------------------------------

  getQuestion(entryId: string): Promise<Question | null> {
    return invoke('get_question', { entryId });
  }

  askQuestion(entryId: string, span?: Span | null): Promise<Question> {
    return invoke('ask_question', { entryId, span: span ?? null });
  }

  runProbe(entryId: string, probeId: string, span?: Span | null): Promise<Question> {
    return invoke('run_probe', { entryId, probeId, span: span ?? null });
  }

  dismissQuestion(entryId: string, questionId: string): Promise<void> {
    return invoke('dismiss_question', { entryId, questionId });
  }

  listProposedEdges(entryId: string): Promise<Edge[]> {
    return invoke('list_proposed_edges', { entryId });
  }

  dismissEdge(edgeId: string): Promise<void> {
    return invoke('dismiss_edge', { edgeId });
  }

  acceptEdge(edgeId: string): Promise<void> {
    return invoke('accept_edge', { edgeId });
  }

  createManualEdge(a: string, b: string, relation: Edge['relation']): Promise<Edge> {
    return invoke('create_manual_edge', { entryA: a, entryB: b, relation });
  }

  // -- action items -------------------------------------------------------

  listActionItems(): Promise<ActionItem[]> {
    return invoke('list_action_items');
  }

  setActionItemDone(id: string, done: boolean): Promise<void> {
    return invoke('set_action_item_done', { id, done });
  }

  // -- resolution ---------------------------------------------------------

  resolveEntry(entryId: string, text: string): Promise<Entry> {
    return invoke('resolve_entry', { entryId, text });
  }

  reopenEntry(entryId: string): Promise<Entry> {
    return invoke('reopen_entry', { entryId });
  }

  // -- system -------------------------------------------------------------

  getSystemProfile(): Promise<SystemProfile> {
    return invoke('get_system_profile');
  }

  listModels(): Promise<ModelInfo[]> {
    return invoke('list_models');
  }

  downloadModel(modelId: string): Promise<void> {
    return invoke('download_model', { modelId });
  }

  onModelProgress(cb: (m: ModelInfo) => void): Unsubscribe {
    return subscribe<ModelInfo>('model://progress', cb);
  }

  getSettings(): Promise<Settings> {
    return invoke('get_settings');
  }

  setSettings(patch: Partial<Settings>): Promise<Settings> {
    return invoke('set_settings', { patch });
  }

  // -- sample corpus ------------------------------------------------------

  loadSampleCorpus(): Promise<void> {
    return invoke('load_sample_corpus');
  }

  clearSampleCorpus(): Promise<void> {
    return invoke('clear_sample_corpus');
  }

  importCorpus(data: CorpusImport, mode: ImportMode): Promise<void> {
    return invoke('import_corpus', { data, mode });
  }
}
