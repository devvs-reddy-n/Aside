import type { SVGProps } from "react";
import {
  ArrowDown, ArrowRight, ArrowUp, AudioLines, Bot, Briefcase, Building2, Camera, Check, ChevronDown, ChevronLeft, ChevronRight, ChevronUp,
  CircleAlert, CircleCheck, Clock, Code, Command, Copy, CopyPlus, Download, Ellipsis, ExternalLink, Eye, File, FileText, Files, Folder,
  History, Info, Keyboard, LayoutGrid, LayoutList, LibraryBig, Lightbulb, Link, List, LoaderCircle, Maximize2, MessageCircleQuestion, MessageSquare, MessagesSquare, Mic, Minus, Monitor, Moon,
  PanelLeft, Pause, Pencil, Play, Plus, RefreshCw, ScanText, ScrollText, Search, Settings, Shield, Sparkles, Square, Sun, TriangleAlert, Trash2,
  Undo2, Upload, Users, Volume2, WifiOff, X, Zap, type LucideIcon,
} from "lucide-react";

/**
 * Aside's icons: Lucide line icons at 1.5 stroke, one icon per idea. Screens
 * ask for an icon by a short name, so swapping a glyph never touches them.
 * Only Aside's own mark is drawn here.
 */
const ICONS = {
  spark: Zap,
  sparkle: Sparkles,
  doc: FileText,
  note: Files,
  library: LibraryBig,
  list: List,
  grid: LayoutGrid,
  clock: Clock,
  history: History,
  gear: Settings,
  plus: Plus,
  minus: Minus,
  search: Search,
  mic: Mic,
  speaker: Volume2,
  camera: Camera,
  chat: MessageSquare,
  send: ArrowUp,
  more: Ellipsis,
  x: X,
  check: Check,
  checkCircle: CircleCheck,
  arrow: ArrowRight,
  play: Play,
  pause: Pause,
  stop: Square,
  copy: Copy,
  duplicate: CopyPlus,
  alert: TriangleAlert,
  alertCircle: CircleAlert,
  refresh: RefreshCw,
  wifioff: WifiOff,
  shield: Shield,
  down: ArrowDown,
  chevronRight: ChevronRight,
  chevronLeft: ChevronLeft,
  chevronDown: ChevronDown,
  chevronUp: ChevronUp,
  code: Code,
  trash: Trash2,
  edit: Pencil,
  download: Download,
  upload: Upload,
  external: ExternalLink,
  eye: Eye,
  link: Link,
  info: Info,
  sun: Sun,
  moon: Moon,
  users: Users,
  expand: Maximize2,
  briefcase: Briefcase,
  keyboard: Keyboard,
  undo: Undo2,
  wave: AudioLines,
  command: Command,
  sidebar: PanelLeft,
  loader: LoaderCircle,
  screen: Monitor,
  building: Building2,
  bot: Bot,
  folder: Folder,
  bulb: Lightbulb,
  file: File,
  scroll: ScrollText,
  question: MessageCircleQuestion,
  layoutFocus: LayoutList,
  layoutThread: MessagesSquare,
  layoutPrompter: ScanText,
} satisfies Record<string, LucideIcon>;

export type IconName = keyof typeof ICONS | "mark";

export interface IconProps extends Omit<SVGProps<SVGSVGElement>, "name" | "ref"> {
  name: IconName;
  size?: number;
}

export function Icon({ name, size = 16, ...rest }: IconProps) {
  if (name === "mark") {
    return (
      <svg className="icon" width={size} height={size} viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false" {...rest}>
        <path d="M2 12.6 11.9 2 11.9 13.8 8.9 8.3" />
      </svg>
    );
  }
  const Glyph = ICONS[name];
  return <Glyph className="icon" size={size} strokeWidth={1.5} aria-hidden="true" focusable="false" {...rest} />;
}
