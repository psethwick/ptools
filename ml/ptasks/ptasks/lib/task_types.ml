type task_options = {
  cwd : string option;
  env : (string * string) list option;
}

type task = {
  label : string;
  task_type : string option;
  command : string option;
  args : string list option;
  options : task_options option;
  depends_on : string list option;
  depends_order : string option;
}

type tasks_file = { tasks : task list }

let string_of_task task = task.label

let find_task_by_label tasks label =
  List.find_opt (fun t -> String.equal t.label label) tasks
