/** Typed Tauri bridge calls for project registration. */

import { invoke } from "@tauri-apps/api/core";
import type {
  Project,
  ProjectListResponse,
  ProjectRegisterRequest,
  ProjectRemoveRequest,
  ProjectRemoveResponse,
} from "@muxyard/contracts";

export async function listProjects(): Promise<ProjectListResponse> {
  return invoke<ProjectListResponse>("project_list");
}

export async function registerProject(
  request: ProjectRegisterRequest,
): Promise<Project> {
  return invoke<Project>("project_register", { request });
}

export async function removeProject(
  request: ProjectRemoveRequest,
): Promise<ProjectRemoveResponse> {
  return invoke<ProjectRemoveResponse>("project_remove", { request });
}
