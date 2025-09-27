open Printf
open Task_types
module Types = Task_types
module Parser = Json_parser
module Executor = Task_executor

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

(** Main run function *)
let run_task_runner ?(tasks_file = ".vscode/tasks.json") () =
  match Parser.read_tasks_from_file tasks_file with
  | Error msg ->
      eprintf "Error: %s\n%!" msg;
      exit 1
  | Ok tasks_file_content -> (
      let tasks = tasks_file_content.tasks in

      if List.length tasks = 0 then (
        printf "No tasks found in %s\n%!" tasks_file;
        exit 1);

      match select_task tasks with
      | None ->
          printf "\nNo task selected. Exiting.\n%!";
          exit 1
      | Some selected_task ->
          Lwt_main.run
            (Lwt.catch
               (fun () ->
                 let%lwt () = Executor.execute_task selected_task tasks in
                 printf "\nTask completed successfully!\n%!";
                 Lwt.return_unit)
               (fun exn ->
                 eprintf "\nError running task '%s':\n%s\n%!"
                   selected_task.label (Printexc.to_string exn);
                 exit 1)))
