let parse_args () =
  let tasks_file = ref ".vscode/tasks.json" in
  let help = ref false in

  let usage_msg =
    "ptasks [OPTIONS]\n\nA task runner for VS Code tasks.json files"
  in

  let spec =
    [
      ( "-f",
        Arg.Set_string tasks_file,
        " Path to tasks.json file (default: .vscode/tasks.json)" );
      ( "--file",
        Arg.Set_string tasks_file,
        " Path to tasks.json file (default: .vscode/tasks.json)" );
      ("-h", Arg.Set help, " Show this help message");
      ("--help", Arg.Set help, " Show this help message");
    ]
  in

  Arg.parse spec (fun _ -> ()) usage_msg;

  if !help then (
    Arg.usage spec usage_msg;
    exit 0);

  !tasks_file

let () =
  let tasks_file = parse_args () in
  Ptasks.run_task_runner ~tasks_file ()
