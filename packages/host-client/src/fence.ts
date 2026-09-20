// ABOUTME: The fence a worker presents on every Host command for an assignment: job, owner and epoch.
// ABOUTME: Shared by the lease keeper, process runtimes and judgement so it is derived in one place.
import type { Assignment } from "../../../contracts/generated/assignment.js";
import type { Fence } from "../../../contracts/generated/host-request.js";

export const fenceFor = (assignment: Assignment): Fence => ({
  job_id: assignment.job.id,
  owner_id: assignment.owner_id,
  epoch: assignment.epoch,
});
