open Task_types

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
    let json = Yojson_five.Safe.from_string content in
    Ok (tasks_file_of_json json)
  with
  | Sys_error msg -> Error ("Failed to read tasks.json: " ^ msg)
  | Yojson.Json_error msg -> Error ("Failed to parse JSON5: " ^ msg)
  | Yojson.Safe.Util.Type_error (msg, _) -> Error ("JSON parsing error: " ^ msg)
  | exn -> Error ("Unexpected error: " ^ Printexc.to_string exn)
