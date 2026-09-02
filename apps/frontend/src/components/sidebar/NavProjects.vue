<script setup lang="ts">
import {
  PhCaretRight,
  PhFolderOpen,
  PhGear,
  PhGitPullRequest,
  PhListChecks,
  PhPlus,
  PhTag,
  PhUser,
} from '@phosphor-icons/vue';
import { computed } from 'vue';
import type { components } from '@/generated/api';

import { Button } from '@/components/ui/button';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible';
import {
  SidebarGroup,
  SidebarGroupAction,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
} from '@/components/ui/sidebar';

export type ProjectNavItem = components['schemas']['ProjectResponse'];

const props = defineProps<{
  tenantSlug: string;
  projects: components['schemas']['ProjectResponse'][];
  /** 現在の URL パス。active 強調と初期展開の判定に使う */
  currentPath?: string;
  loading?: boolean;
  error?: boolean;
}>();

const emit = defineEmits<{
  retry: [];
  create: [];
}>();

const sortedProjects = computed(() =>
  [...props.projects].sort((a, b) => {
    if (a.is_personal === b.is_personal) return a.name.localeCompare(b.name);
    return a.is_personal ? -1 : 1;
  }),
);

function projectBaseUrl(project: ProjectNavItem) {
  return `/${props.tenantSlug}/projects/${project.key}`;
}

/** プロジェクト配下のページを開いているか（初期展開・親の active 判定） */
function isProjectActive(project: ProjectNavItem) {
  return !!props.currentPath && props.currentPath.startsWith(`${projectBaseUrl(project)}/`);
}

interface ProjectChild {
  label: string;
  href: string;
  icon: typeof PhListChecks;
}

function projectChildren(project: ProjectNavItem): ProjectChild[] {
  const base = projectBaseUrl(project);
  const children: ProjectChild[] = [
    { label: 'タスク', href: `${base}/tasks`, icon: PhListChecks },
    { label: 'ラベル', href: `${base}/labels`, icon: PhTag },
  ];
  // 個人プロジェクト（個人 Inbox）はシステム管理のため設定を出さない。
  // レビューも、設定へ入れない＝GitHub 連携を張る手段が無いので出さない
  if (!project.is_personal) {
    children.push({ label: 'レビュー', href: `${base}/reviews`, icon: PhGitPullRequest });
    children.push({ label: '設定', href: `${base}/settings`, icon: PhGear });
  }
  return children;
}

function isChildActive(child: ProjectChild) {
  // 完全一致 or セグメント境界付き前方一致（tasks と tasks-archive のような
  // 接頭辞衝突を防ぎつつ、配下ページ /…/tasks/T-1 でも「タスク」を active に保つ）
  return (
    !!props.currentPath &&
    (props.currentPath === child.href || props.currentPath.startsWith(`${child.href}/`))
  );
}
</script>

<template>
  <SidebarGroup class="group-data-[collapsible=icon]:hidden">
    <SidebarGroupLabel>Projects</SidebarGroupLabel>
    <SidebarGroupAction
      v-if="tenantSlug"
      as="button"
      type="button"
      title="プロジェクトを作成"
      @click="emit('create')"
    >
      <PhPlus />
      <span class="sr-only">プロジェクトを作成</span>
    </SidebarGroupAction>
    <SidebarMenu v-if="loading">
      <SidebarMenuItem>
        <SidebarMenuButton disabled>
          <span class="text-muted-foreground text-sm">プロジェクトを読み込み中…</span>
        </SidebarMenuButton>
      </SidebarMenuItem>
    </SidebarMenu>
    <SidebarMenu v-else-if="error">
      <SidebarMenuItem>
        <SidebarMenuButton class="text-destructive" @click="emit('retry')">
          プロジェクト一覧を取得できませんでした（再試行）
        </SidebarMenuButton>
      </SidebarMenuItem>
    </SidebarMenu>
    <div v-else-if="sortedProjects.length === 0" class="p-2">
      <div class="rounded-lg border border-dashed px-3 py-3.5 text-center">
        <p class="mb-2.5 text-xs leading-snug text-muted-foreground">
          プロジェクトはまだありません。
        </p>
        <Button v-if="tenantSlug" size="sm" class="h-8 w-full text-[13px]" @click="emit('create')">
          <PhPlus class="size-3.5" />
          プロジェクトを作成
        </Button>
      </div>
    </div>
    <SidebarMenu v-else>
      <Collapsible
        v-for="project in sortedProjects"
        :key="project.id"
        as-child
        :default-open="isProjectActive(project)"
        class="group/collapsible"
      >
        <SidebarMenuItem>
          <CollapsibleTrigger as-child>
            <SidebarMenuButton :is-active="isProjectActive(project)">
              <img
                v-if="project.icon_url"
                :src="project.icon_url"
                :alt="project.name"
                class="size-4 shrink-0 rounded-sm object-cover"
              />
              <span v-else-if="project.icon_emoji" class="text-base leading-none">{{
                project.icon_emoji
              }}</span>
              <PhUser v-else-if="project.is_personal" />
              <PhFolderOpen v-else />
              <span>{{ project.name }}</span>
              <PhCaretRight
                class="ml-auto transition-transform duration-200 group-data-[state=open]/collapsible:rotate-90"
              />
            </SidebarMenuButton>
          </CollapsibleTrigger>
          <CollapsibleContent>
            <SidebarMenuSub v-if="tenantSlug">
              <SidebarMenuSubItem v-for="child in projectChildren(project)" :key="child.href">
                <SidebarMenuSubButton as-child :is-active="isChildActive(child)">
                  <a :href="child.href">
                    <component :is="child.icon" />
                    <span>{{ child.label }}</span>
                  </a>
                </SidebarMenuSubButton>
              </SidebarMenuSubItem>
            </SidebarMenuSub>
          </CollapsibleContent>
        </SidebarMenuItem>
      </Collapsible>
    </SidebarMenu>
  </SidebarGroup>
</template>
