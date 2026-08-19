export type User = {
  id: string;
  username: string;
  email: string;
};

export type Tenant = {
  id: string;
  name: string;
  display_id: string;
};

export type Project = {
  id: string;
  tenant_id: string;
  name: string;
  key: string;
  description?: string | null;
  is_personal?: boolean;
};

export type TaskPriority =
  | "critical_fire"
  | "critical"
  | "high"
  | "medium"
  | "low"
  | "trivial";

export type Task = {
  id: string;
  project_id: string;
  seq_id: number;
  title: string;
  description?: string | null;
  status_id: string;
  priority: TaskPriority;
  sprint_id?: string | null;
  soft_deadline?: string | null;
  hard_deadline?: string | null;
  completed_at?: string | null;
};

export type TaskListResponse = {
  tasks: Task[];
  total: number;
};

export type ProjectStatus = {
  id: string;
  project_id: string;
  name: string;
  color: string;
  is_done_state: boolean;
};

export type Sprint = {
  id: string;
  project_id: string;
  name: string;
  goal?: string | null;
  status: string;
  start_date: string;
  end_date: string;
};

export type SprintDetail = {
  sprint: Sprint;
  task_counts: { total: number; done: number; in_progress: number };
  burndown: Array<{
    date: string;
    ideal_remaining: number;
    actual_remaining: number;
  }>;
};

export type MyTaskItem = {
  id: string;
  seq_id: number;
  seq_key: string;
  title: string;
  project: { id: string; name: string; key: string; is_personal: boolean };
  status: { id: string; name: string; color: string };
};

export type MyTasksListResponse = {
  tasks: MyTaskItem[];
  total: number;
};

export type Comment = {
  id: string;
  task_id: string;
  body: string;
  user_id: string;
  created_at: string;
};

export type SearchTaskHit = {
  id: string;
  seq_id: number;
  title: string;
  highlight: string;
  score: number;
};

export type SearchTasksResponse = {
  tasks: SearchTaskHit[];
  total: number;
};

export type BulkUpdateFields = {
  status_id?: string;
  assignee_id?: string;
  add_label_ids?: string[];
  sprint_id?: string;
  clear_sprint_id?: boolean;
};

export type BulkUpdateRequest = {
  task_ids: string[];
  update: BulkUpdateFields;
};

export type BulkFailure = {
  task_id: string;
  reason: string;
};

export type BulkUpdateResponse = {
  updated: number;
  failed: BulkFailure[];
};

export type TaskView = {
  id: string;
  project_id: string;
  created_by: string;
  name: string;
  is_shared: boolean;
  filters: Record<string, unknown>;
  sort: Record<string, unknown>;
  view_type: string;
  created_at: string;
};

export type TaskViewListResponse = {
  views: TaskView[];
};

export type TaskAttachment = {
  id: string;
  drive_file_id: string;
  name: string;
  mime_type: string;
  size: number;
  url: string;
  created_at: string;
};

export type TaskAttachmentListResponse = {
  attachments: TaskAttachment[];
};

export interface ApiPaths {
  "/v1/auth/me": {
    get: {
      responses: {
        200: { content: { "application/json": User } };
      };
    };
  };
  "/v1/tenants": {
    get: {
      responses: {
        200: { content: { "application/json": Tenant[] } };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects": {
    get: {
      parameters: { path: { tenant_id: string } };
      responses: {
        200: { content: { "application/json": Project[] } };
      };
    };
    post: {
      parameters: { path: { tenant_id: string } };
      requestBody: {
        content: {
          "application/json": {
            name: string;
            description?: string;
            key?: string;
          };
        };
      };
      responses: {
        201: { content: { "application/json": Project } };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects/{id}": {
    get: {
      parameters: { path: { tenant_id: string; id: string } };
      responses: {
        200: { content: { "application/json": Project } };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects/{project_id}/tasks": {
    get: {
      parameters: {
        path: { tenant_id: string; project_id: string };
        query?: {
          status_id?: string;
          priority?: string;
          limit?: number;
          offset?: number;
        };
      };
      responses: {
        200: { content: { "application/json": TaskListResponse } };
      };
    };
    post: {
      parameters: { path: { tenant_id: string; project_id: string } };
      requestBody: {
        content: {
          "application/json": {
            title: string;
            description?: string;
            priority?: TaskPriority;
            status_id?: string;
          };
        };
      };
      responses: {
        201: { content: { "application/json": Task } };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}": {
    get: {
      parameters: {
        path: { tenant_id: string; project_id: string; id: string };
      };
      responses: {
        200: { content: { "application/json": Task } };
      };
    };
    put: {
      parameters: {
        path: { tenant_id: string; project_id: string; id: string };
      };
      requestBody: {
        content: {
          "application/json": {
            title?: string;
            description?: string;
            status_id?: string;
            priority?: TaskPriority;
          };
        };
      };
      responses: {
        200: { content: { "application/json": Task } };
      };
    };
    delete: {
      parameters: {
        path: { tenant_id: string; project_id: string; id: string };
      };
      responses: {
        204: { content: never };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects/{project_id}/tasks/search": {
    get: {
      parameters: {
        path: { tenant_id: string; project_id: string };
        query: { q: string; limit?: number; offset?: number };
      };
      responses: {
        200: { content: { "application/json": SearchTasksResponse } };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects/{project_id}/tasks/bulk": {
    post: {
      parameters: { path: { tenant_id: string; project_id: string } };
      requestBody: {
        content: { "application/json": BulkUpdateRequest };
      };
      responses: {
        200: { content: { "application/json": BulkUpdateResponse } };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects/{project_id}/task-views": {
    get: {
      parameters: { path: { tenant_id: string; project_id: string } };
      responses: {
        200: { content: { "application/json": TaskViewListResponse } };
      };
    };
    post: {
      parameters: { path: { tenant_id: string; project_id: string } };
      requestBody: {
        content: {
          "application/json": {
            name: string;
            is_shared?: boolean;
            filters?: Record<string, unknown>;
            sort?: Record<string, unknown>;
            view_type?: string;
          };
        };
      };
      responses: {
        201: { content: { "application/json": TaskView } };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects/{project_id}/task-views/{view_id}": {
    patch: {
      parameters: {
        path: { tenant_id: string; project_id: string; view_id: string };
      };
      requestBody: {
        content: {
          "application/json": {
            name?: string;
            is_shared?: boolean;
            filters?: Record<string, unknown>;
            sort?: Record<string, unknown>;
            view_type?: string;
          };
        };
      };
      responses: {
        200: { content: { "application/json": TaskView } };
      };
    };
    delete: {
      parameters: {
        path: { tenant_id: string; project_id: string; view_id: string };
      };
      responses: {
        204: { content: never };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}/attachments": {
    get: {
      parameters: {
        path: { tenant_id: string; project_id: string; id: string };
      };
      responses: {
        200: { content: { "application/json": TaskAttachmentListResponse } };
      };
    };
    post: {
      parameters: {
        path: { tenant_id: string; project_id: string; id: string };
      };
      requestBody: {
        content: { "application/json": { drive_file_id: string } };
      };
      responses: {
        201: { content: { "application/json": TaskAttachment } };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}/attachments/{attachment_id}": {
    delete: {
      parameters: {
        path: {
          tenant_id: string;
          project_id: string;
          id: string;
          attachment_id: string;
        };
      };
      responses: {
        204: { content: never };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects/{project_id}/tasks/{id}/comments": {
    post: {
      parameters: {
        path: { tenant_id: string; project_id: string; id: string };
      };
      requestBody: {
        content: { "application/json": { body: string } };
      };
      responses: {
        201: { content: { "application/json": Comment } };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects/{project_id}/statuses": {
    get: {
      parameters: { path: { tenant_id: string; project_id: string } };
      responses: {
        200: { content: { "application/json": ProjectStatus[] } };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects/{project_id}/sprints": {
    get: {
      parameters: {
        path: { tenant_id: string; project_id: string };
        query?: { status?: string };
      };
      responses: {
        200: { content: { "application/json": Sprint[] } };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects/{project_id}/sprints/{id}": {
    get: {
      parameters: {
        path: { tenant_id: string; project_id: string; id: string };
      };
      responses: {
        200: { content: { "application/json": SprintDetail } };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects/{project_id}/sprints/{id}/start": {
    post: {
      parameters: {
        path: { tenant_id: string; project_id: string; id: string };
      };
      responses: {
        200: { content: { "application/json": Sprint } };
      };
    };
  };
  "/v1/tenants/{tenant_id}/projects/{project_id}/sprints/{id}/complete": {
    post: {
      parameters: {
        path: { tenant_id: string; project_id: string; id: string };
      };
      requestBody?: {
        content: {
          "application/json": {
            move_incomplete_to_backlog?: boolean;
            move_incomplete_to_sprint_id?: string;
          };
        };
      };
      responses: {
        200: { content: { "application/json": Sprint } };
      };
    };
  };
  "/v1/tenants/{tenant_id}/users/me/tasks": {
    get: {
      parameters: {
        path: { tenant_id: string };
        query?: { filter?: string; limit?: number; offset?: number };
      };
      responses: {
        200: { content: { "application/json": MyTasksListResponse } };
      };
    };
  };
}
