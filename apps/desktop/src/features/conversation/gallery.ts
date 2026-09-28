import { createContext } from "react";
import type { LightboxItem } from "../../lib/lightbox";

/** Returns the open conversation's viewable attachments (loaded so far), oldest first. */
export const GalleryContext = createContext<() => LightboxItem[]>(() => []);
