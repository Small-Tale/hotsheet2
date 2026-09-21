import {
  ArrowLeftRight,
  Balloon,
  Bot,
  Bug,
  CircleCheckBig,
  Cloud,
  Code,
  Database,
  Download,
  FileText,
  FlaskConical,
  FolderGit2,
  GitBranch,
  GitCompare,
  GitCompareArrows,
  GitMerge,
  GitPullRequest,
  Globe,
  Hammer,
  type IconNode,
  ListChecks,
  Package,
  Play,
  RefreshCw,
  Rocket,
  Search,
  Send,
  Settings,
  Shield,
  SoapDispenserDroplet,
  Sparkles,
  SquareTerminal,
  TestTube2,
  Upload,
  Wand,
  Wrench,
  Zap,
} from 'lucide';

/** A bundled, always-available icon: its kebab-case name and Lucide node. */
export interface PopularLucideIcon {
  name: string;
  icon: IconNode;
}

/**
 * Curated command-button icons kept in the main bundle so the picker's default set and every legacy
 * command icon render synchronously without waiting for the full lazy Lucide catalog. Includes the
 * original HS1 command palette plus common developer-workflow icons.
 */
export const POPULAR_LUCIDE_ICONS: readonly PopularLucideIcon[] = [
  { name: 'send', icon: Send },
  { name: 'square-terminal', icon: SquareTerminal },
  { name: 'play', icon: Play },
  { name: 'hammer', icon: Hammer },
  { name: 'wrench', icon: Wrench },
  { name: 'test-tube-2', icon: TestTube2 },
  { name: 'flask-conical', icon: FlaskConical },
  { name: 'circle-check-big', icon: CircleCheckBig },
  { name: 'list-checks', icon: ListChecks },
  { name: 'bug', icon: Bug },
  { name: 'file-text', icon: FileText },
  { name: 'code', icon: Code },
  { name: 'git-branch', icon: GitBranch },
  { name: 'git-compare', icon: GitCompare },
  { name: 'git-compare-arrows', icon: GitCompareArrows },
  { name: 'git-merge', icon: GitMerge },
  { name: 'git-pull-request', icon: GitPullRequest },
  { name: 'folder-git-2', icon: FolderGit2 },
  { name: 'arrow-left-right', icon: ArrowLeftRight },
  { name: 'refresh-cw', icon: RefreshCw },
  { name: 'upload', icon: Upload },
  { name: 'download', icon: Download },
  { name: 'package', icon: Package },
  { name: 'rocket', icon: Rocket },
  { name: 'search', icon: Search },
  { name: 'settings', icon: Settings },
  { name: 'database', icon: Database },
  { name: 'cloud', icon: Cloud },
  { name: 'shield', icon: Shield },
  { name: 'zap', icon: Zap },
  { name: 'sparkles', icon: Sparkles },
  { name: 'wand', icon: Wand },
  { name: 'bot', icon: Bot },
  { name: 'globe', icon: Globe },
  { name: 'balloon', icon: Balloon },
  { name: 'soap-dispenser-droplet', icon: SoapDispenserDroplet },
];

export const POPULAR_LUCIDE_MAP = new Map(POPULAR_LUCIDE_ICONS.map((entry) => [entry.name, entry.icon] as const));
