open Printf

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

let task_options_of_json json =
  let open Yojson.Safe.Util in
  {
    cwd = json |> member "cwd" |> to_string_option;
    env =
      json |> member "env"
      |> to_option (fun env_json ->
             env_json |> to_assoc |> List.map (fun (k, v) -> (k, to_string v)));
  }

let task_of_json json =
  let open Yojson.Safe.Util in
  {
    label = json |> member "label" |> to_string;
    task_type = json |> member "type" |> to_string_option;
    command = json |> member "command" |> to_string_option;
    args = json |> member "args" |> to_option (convert_each to_string);
    options = json |> member "options" |> to_option task_options_of_json;
    depends_on =
      json |> member "dependsOn" |> to_option (convert_each to_string);
    depends_order = json |> member "dependsOrder" |> to_string_option;
  }

let tasks_file_of_json json =
  let open Yojson.Safe.Util in
  { tasks = json |> member "tasks" |> to_list |> List.map task_of_json }

let read_tasks_from_file path =
  try
    let content = In_channel.with_open_text path In_channel.input_all in
    match Yojson_five.Safe.from_string content with
    | Ok json -> Ok (tasks_file_of_json json)
    | Error msg -> Error ("Failed to parse JSON5: " ^ msg)
  with
  | Sys_error msg -> Error ("Failed to read tasks.json: " ^ msg)
  | Yojson.Safe.Util.Type_error (msg, _) -> Error ("JSON parsing error: " ^ msg)
  | exn -> Error ("Unexpected error: " ^ Printexc.to_string exn)

(* Terminal title setting *)
let set_window_title title = printf "\027]0;%s\007%!" title

(* Build command and arguments based on task type *)
let build_command task command_name =
  match task.task_type with
  | Some "process" ->
      let args =
        match task.args with
        | None -> []
        | Some args -> args
      in
      (command_name, args)
  | _ ->
      (* Default to shell *)
      let script =
        match task.args with
        | None -> command_name
        | Some args ->
            let arg_placeholders =
              List.mapi (fun i _ -> sprintf "\"$%d\"" (i + 1)) args
              |> String.concat " "
            in
            command_name ^ " " ^ arg_placeholders
      in
      let shell_args =
        [ "-c"; script; command_name ]
        @
        match task.args with
        | None -> []
        | Some args -> args
      in
      ("sh", shell_args)

(* Build environment variables array *)
let build_environment task =
  match task.options with
  | None -> None
  | Some options -> (
      match options.env with
      | None -> None
      | Some env_list ->
          let current_env = Unix.environment () |> Array.to_list in
          let new_env = List.map (fun (k, v) -> k ^ "=" ^ v) env_list in
          let env_array = current_env @ new_env |> Array.of_list in
          Some env_array)

(* Get working directory from task options *)
let get_working_directory task =
  match task.options with
  | None -> None
  | Some options -> options.cwd

(* Execute a command and return exit code *)
let execute_command task command_name =
  let cmd, args = build_command task command_name in
  let env = build_environment task in
  let cwd = get_working_directory task in

  let process_args = Array.of_list (cmd :: args) in

  let process = Lwt_process.open_process_none ?cwd ?env (cmd, process_args) in
  let%lwt status = process#close in

  match status with
  | Unix.WEXITED code -> Lwt.return code
  | Unix.WSIGNALED _ -> Lwt.return 128
  | Unix.WSTOPPED _ -> Lwt.return 128

(* Execute dependencies sequentially *)
let rec execute_dependencies_sequential dependencies all_tasks =
  let rec execute_deps = function
    | [] -> Lwt.return_unit
    | dep_label :: rest -> (
        match List.find_opt (fun t -> t.label = dep_label) all_tasks with
        | None ->
            Lwt.fail_with (sprintf "Dependent task '%s' not found" dep_label)
        | Some dep_task ->
            printf "\n--> Running dependent task: %s\n%!" dep_label;
            let%lwt () = execute_task dep_task all_tasks in
            printf "\n<-- Finished dependent task: %s\n%!" dep_label;
            execute_deps rest)
  in
  execute_deps dependencies

(* Execute dependencies in parallel *)
and execute_dependencies_parallel dependencies all_tasks =
  let execute_single_dep dep_label =
    match List.find_opt (fun t -> t.label = dep_label) all_tasks with
    | None -> Lwt.fail_with (sprintf "Dependent task '%s' not found" dep_label)
    | Some dep_task ->
        printf "\n--> Spawning dependent task: %s\n%!" dep_label;
        let%lwt () = execute_task dep_task all_tasks in
        printf "\n<-- Finished dependent task: %s\n%!" dep_label;
        Lwt.return_unit
  in
  Lwt.join (List.map execute_single_dep dependencies)

and handle_dependencies task all_tasks =
  match task.depends_on with
  | None -> Lwt.return_unit
  | Some dependencies -> (
      match task.depends_order with
      | Some "parallel" -> execute_dependencies_parallel dependencies all_tasks
      | _ -> execute_dependencies_sequential dependencies all_tasks)

and execute_task task all_tasks =
  let%lwt () = handle_dependencies task all_tasks in

  set_window_title task.label;

  match task.command with
  | None -> Lwt.fail_with "Task has no command to execute"
  | Some command_name ->
      let%lwt exit_code = execute_command task command_name in

      set_window_title "ptasks";

      if exit_code = 0 then
        Lwt.return_unit
      else
        Lwt.fail_with (sprintf "Command failed with status: %d" exit_code)

let select_task tasks =
  printf "Select a task to run:\n%!";
  List.iteri (fun i task -> printf "%d) %s\n%!" (i + 1) task.label) tasks;
  printf "Enter your choice (1-%d): %!" (List.length tasks);

  let rec get_choice () =
    match read_line () with
    | exception End_of_file -> None
    | input -> (
        try
          let choice = int_of_string (String.trim input) in
          if choice >= 1 && choice <= List.length tasks then
            Some (List.nth tasks (choice - 1))
          else (
            printf "Invalid choice. Enter a number between 1 and %d: %!"
              (List.length tasks);
            get_choice ())
        with Failure _ ->
          printf "Invalid input. Enter a number between 1 and %d: %!"
            (List.length tasks);
          get_choice ())
  in
  get_choice ()

let main () =
  let tasks_json_path = ".vscode/tasks.json" in

  match read_tasks_from_file tasks_json_path with
  | Error msg ->
      eprintf "Error: %s\n%!" msg;
      exit 1
  | Ok tasks_file -> (
      let tasks = tasks_file.tasks in

      if List.length tasks = 0 then (
        printf "No tasks found in %s\n%!" tasks_json_path;
        exit 1);

      match select_task tasks with
      | None ->
          printf "\nNo task selected. Exiting.\n%!";
          exit 1
      | Some selected_task ->
          Lwt_main.run
            (let%lwt () =
               Lwt.catch
                 (fun () -> execute_task selected_task tasks)
                 (fun exn ->
                   eprintf "\nError running task '%s':\n%s\n%!"
                     selected_task.label (Printexc.to_string exn);
                   exit 1)
             in
             printf "\nTask completed successfully!\n%!";
             Lwt.return_unit))

let () = main ()
