'use client';

import { create } from 'zustand';
import { subscribeWithSelector } from 'zustand/middleware';
import { createCameraSlice, type CameraSlice } from './camera';
import { createCaptureSlice, type CaptureSlice } from './capture';
import { createCorpusSlice, type CorpusSlice } from './corpus';
import { createPlaybackSlice, type PlaybackSlice } from './playback';
import { createSearchSlice, type SearchSlice } from './search';
import { createTypesSlice, type TypesSlice } from './types';
import { createUiSlice, type UiSlice } from './ui';

export type AppState = CameraSlice & CaptureSlice & CorpusSlice & PlaybackSlice & SearchSlice & TypesSlice & UiSlice;

/**
 * subscribeWithSelector is not optional — it's what lets the Pixi renderer
 * subscribe (useApp.subscribe) and drive itself outside React entirely.
 * Without it, panning would need to re-render a component.
 */
export type Mutators = [['zustand/subscribeWithSelector', never]];

export const useApp = create<AppState>()(
  subscribeWithSelector((...a) => ({
    ...createCameraSlice(...a),
    ...createCaptureSlice(...a),
    ...createCorpusSlice(...a),
    ...createPlaybackSlice(...a),
    ...createSearchSlice(...a),
    ...createTypesSlice(...a),
    ...createUiSlice(...a),
  })),
);

export type { Camera } from './camera';
export type { CaptureState } from './capture';
export type { Overlay, Theme } from './ui';
